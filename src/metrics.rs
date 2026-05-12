//! Prometheus metrics.

use prometheus::{
    Encoder, Histogram, HistogramOpts, IntCounter, IntCounterVec, IntGauge, Opts, Registry,
    TextEncoder,
};
use std::sync::Arc;

pub struct Metrics {
    pub registry: Registry,
    pub connected_peers: IntGauge,
    pub registrations_total: IntCounter,
    pub handle_claims_total: IntCounterVec, // labels: outcome (ok | conflict | error)
    pub signaling_messages_total: IntCounter,
    pub signaling_rejected_total: IntCounterVec, // labels: reason
    pub relay_bytes_total: IntCounter,
    pub relay_sessions_total: IntCounter,
    pub team_announces_total: IntCounter,
    pub presence_subscriptions: IntGauge,
    pub message_handle_seconds: Histogram,
}

impl Metrics {
    pub fn new() -> Arc<Self> {
        let registry = Registry::new();

        let connected_peers =
            IntGauge::new("broker_connected_peers", "currently-connected peers").unwrap();
        let registrations_total =
            IntCounter::new("broker_registrations_total", "successful registrations").unwrap();
        let handle_claims_total = IntCounterVec::new(
            Opts::new("broker_handle_claims_total", "handle claim attempts"),
            &["outcome"],
        )
        .unwrap();
        let signaling_messages_total = IntCounter::new(
            "broker_signaling_messages_total",
            "signaling messages relayed",
        )
        .unwrap();
        let signaling_rejected_total = IntCounterVec::new(
            Opts::new(
                "broker_signaling_rejected_total",
                "signaling messages rejected",
            ),
            &["reason"],
        )
        .unwrap();
        let relay_bytes_total =
            IntCounter::new("broker_relay_bytes_total", "bytes forwarded via relay").unwrap();
        let relay_sessions_total =
            IntCounter::new("broker_relay_sessions_total", "relay sessions established").unwrap();
        let team_announces_total =
            IntCounter::new("broker_team_announces_total", "team membership announces").unwrap();
        let presence_subscriptions =
            IntGauge::new("broker_presence_subscriptions", "active presence watchers").unwrap();
        let message_handle_seconds = Histogram::with_opts(HistogramOpts::new(
            "broker_message_handle_seconds",
            "time spent handling one client message",
        ))
        .unwrap();

        registry.register(Box::new(connected_peers.clone())).unwrap();
        registry.register(Box::new(registrations_total.clone())).unwrap();
        registry.register(Box::new(handle_claims_total.clone())).unwrap();
        registry.register(Box::new(signaling_messages_total.clone())).unwrap();
        registry.register(Box::new(signaling_rejected_total.clone())).unwrap();
        registry.register(Box::new(relay_bytes_total.clone())).unwrap();
        registry.register(Box::new(relay_sessions_total.clone())).unwrap();
        registry.register(Box::new(team_announces_total.clone())).unwrap();
        registry.register(Box::new(presence_subscriptions.clone())).unwrap();
        registry.register(Box::new(message_handle_seconds.clone())).unwrap();

        Arc::new(Self {
            registry,
            connected_peers,
            registrations_total,
            handle_claims_total,
            signaling_messages_total,
            signaling_rejected_total,
            relay_bytes_total,
            relay_sessions_total,
            team_announces_total,
            presence_subscriptions,
            message_handle_seconds,
        })
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        let encoder = TextEncoder::new();
        let metric_families = self.registry.gather();
        encoder.encode(&metric_families, &mut buf).unwrap();
        buf
    }
}
