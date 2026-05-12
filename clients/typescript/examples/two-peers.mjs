// Two-peer end-to-end check, exercises register + signal + relay round-trip.
// Mirrors what smoke-client.rs does. Run with:
//   BROKER_URL=ws://localhost:8080/ws node ./examples/two-peers.mjs
//
// (Assumes the SDK has been built into ./dist/.)

import { BrokerClient, generateKeypair } from "../dist/index.js";

const BROKER_URL = process.env.BROKER_URL ?? "ws://localhost:8080/ws";

function b64(s) {
  return Buffer.from(s, "utf8").toString("base64");
}
function b64decode(s) {
  return Buffer.from(s, "base64").toString("utf8");
}

async function makePeer(label, addr) {
  const kp = await generateKeypair();
  const client = new BrokerClient({
    url: BROKER_URL,
    privateKey: kp.privateKey,
    publicKey: kp.publicKey,
  });
  await client.connect();
  const reg = await client.register({
    addresses: { lan: [addr], public: null },
  });
  console.log(`[${label}] registered: peer_id=${reg.peer_id.slice(0, 16)} reflected=${reg.reflected_address}`);
  return { client, kp };
}

async function main() {
  console.log(`[two-peers] broker = ${BROKER_URL}`);

  const alice = await makePeer("alice", "192.168.1.10:51820");
  const bob = await makePeer("bob", "192.168.1.20:51820");

  // ---- Signaling: alice -> bob ------------------------------------------
  const signalReceived = new Promise((resolve) => {
    const off = bob.client.onSignal((msg) => {
      off();
      resolve(msg);
    });
  });
  await alice.client.sendSignal(bob.kp.publicKeyHex, b64("hello bob from alice"));
  const got = await signalReceived;
  if (b64decode(got.payload) !== "hello bob from alice") {
    throw new Error(`unexpected signal payload: ${got.payload}`);
  }
  if (got.from !== alice.kp.publicKeyHex) {
    throw new Error(`signal from mismatch: ${got.from}`);
  }
  console.log("[two-peers] signaling alice->bob OK");

  // ---- Relay: alice opens, bob accepts ---------------------------------
  const offer = new Promise((resolve) => {
    const off = bob.client.onRelayOffer((msg) => {
      off();
      resolve(msg);
    });
  });
  const ack = await alice.client.requestRelay(bob.kp.publicKeyHex);
  const offerMsg = await offer;
  if (offerMsg.session_id !== ack.session_id) {
    throw new Error(`offer session_id ${offerMsg.session_id} != ack ${ack.session_id}`);
  }
  console.log(`[two-peers] relay session ${ack.session_id} offered`);

  // Both peers expect a `relay_session_established` after bob accepts.
  const aliceEstablished = new Promise((resolve) => {
    const off = alice.client.onRelaySessionEstablished((msg) => {
      if (msg.session_id === ack.session_id) {
        off();
        resolve(msg);
      }
    });
  });
  const bobEstablished = new Promise((resolve) => {
    const off = bob.client.onRelaySessionEstablished((msg) => {
      if (msg.session_id === ack.session_id) {
        off();
        resolve(msg);
      }
    });
  });
  await bob.client.acceptRelay(ack.session_id);
  await Promise.all([aliceEstablished, bobEstablished]);
  console.log("[two-peers] relay session active");

  // Alice -> Bob frame
  const bobGot = new Promise((resolve) => {
    const off = bob.client.onRelayData((msg) => {
      off();
      resolve(msg);
    });
  });
  await alice.client.sendRelayFrame(ack.session_id, b64("frame A"));
  const bobFrame = await bobGot;
  if (b64decode(bobFrame.data) !== "frame A") {
    throw new Error(`bob received wrong frame: ${b64decode(bobFrame.data)}`);
  }

  // Bob -> Alice frame
  const aliceGot = new Promise((resolve) => {
    const off = alice.client.onRelayData((msg) => {
      off();
      resolve(msg);
    });
  });
  await bob.client.sendRelayFrame(ack.session_id, b64("frame B"));
  const aliceFrame = await aliceGot;
  if (b64decode(aliceFrame.data) !== "frame B") {
    throw new Error(`alice received wrong frame: ${b64decode(aliceFrame.data)}`);
  }
  console.log("[two-peers] relay bidirectional OK");

  // ---- Ping --------------------------------------------------------------
  const pong = await alice.client.ping();
  console.log(`[two-peers] ping: pong with timestamp=${pong.timestamp}`);

  // ---- Cleanup -----------------------------------------------------------
  await alice.client.close();
  await bob.client.close();
  console.log("[two-peers] ALL CHECKS PASSED");
}

main().catch((err) => {
  console.error("[two-peers] FAILED:", err);
  process.exit(1);
});
