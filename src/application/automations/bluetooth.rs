//! On-demand BLE sampling. Radio work starts only in the long-lived async host.
use arcrelay_automation::{
    AutomationEngine, AutomationEvent, AutomationTrigger, BluetoothDetection, BluetoothDetector,
    BluetoothReading, BluetoothTrigger, BLUETOOTH_STALE_MS,
};
use btleplug::{
    api::{Central, CentralEvent, CentralState, Manager as _, Peripheral as _, ScanFilter},
    platform::{Adapter, Manager},
};
use futures_util::StreamExt;
use serde::Serialize;
use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use ts_rs::TS;

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[derive(Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct BluetoothDeviceView {
    pub id: String,
    pub name: String,
    pub rssi: Option<i16>,
    pub age_seconds: u32,
    pub history: Vec<i16>,
    pub readable: bool,
}
#[derive(Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct BluetoothSnapshot {
    pub status: String,
    pub error: Option<String>,
    pub devices: Vec<BluetoothDeviceView>,
}
#[derive(Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct BluetoothSample {
    pub device_id: String,
    pub rssi: i16,
    pub sample_count: u32,
}
#[derive(Default)]
struct Device {
    name: String,
    seen_at: u64,
    samples: VecDeque<BluetoothReading>,
}
impl Device {
    fn reading(&self, now: u64) -> Option<BluetoothReading> {
        let last = self.samples.back()?;
        let mut values: Vec<_> = self
            .samples
            .iter()
            .rev()
            .take(5)
            .filter(|r| now.saturating_sub(r.at_ms) <= BLUETOOTH_STALE_MS)
            .map(|r| r.rssi)
            .collect();
        // Preserve the real last-seen timestamp even when the sample has expired.
        if values.is_empty() {
            return Some(*last);
        }
        values.sort_unstable();
        Some(BluetoothReading {
            rssi: values[values.len() / 2],
            at_ms: last.at_ms,
        })
    }
}
struct Preview {
    config: BluetoothTrigger,
    detector: BluetoothDetector,
    touched: u64,
}
#[derive(Default)]
struct State {
    status: String,
    error: Option<String>,
    devices: HashMap<String, Device>,
    preview_until: u64,
    enabled: bool,
    epoch: u64,
    previews: HashMap<String, Preview>,
    paused_rules: HashMap<String, u64>,
}
#[derive(Clone, Default)]
pub struct BluetoothService(Arc<Mutex<State>>);
impl BluetoothService {
    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
    fn wanted(&self) -> bool {
        let s = self.lock();
        s.enabled || s.preview_until > now_ms()
    }
    fn status(&self, status: &str, error: Option<String>) {
        let mut s = self.lock();
        if s.status != status {
            s.epoch += 1;
        }
        s.status = status.into();
        s.error = error;
    }
    pub fn reset(&self) {
        let mut s = self.lock();
        s.epoch += 1;
        s.devices.clear();
        s.previews.clear();
    }
    pub fn snapshot(&self, paused_rule: Option<&str>) -> BluetoothSnapshot {
        let now = now_ms();
        let mut s = self.lock();
        s.preview_until = now + 15_000;
        if let Some(id) = paused_rule.filter(|id| id.len() <= 128) {
            s.paused_rules.insert(id.into(), now + 15_000);
        }
        s.paused_rules.retain(|_, until| *until > now);
        let mut devices: Vec<_> = s
            .devices
            .iter()
            .map(|(id, d)| {
                let r = d.reading(now);
                let age = r.map(|v| now.saturating_sub(v.at_ms)).unwrap_or(u64::MAX);
                BluetoothDeviceView {
                    id: id.clone(),
                    name: d.name.clone(),
                    rssi: r.filter(|_| age <= BLUETOOTH_STALE_MS).map(|v| v.rssi),
                    age_seconds: (age / 1000).min(u32::MAX as u64) as u32,
                    history: d
                        .samples
                        .iter()
                        .filter(|r| now.saturating_sub(r.at_ms) <= 60_000)
                        .map(|r| r.rssi)
                        .collect(),
                    readable: age <= BLUETOOTH_STALE_MS && d.samples.len() >= 3,
                }
            })
            .collect();
        devices.sort_by(|a, b| b.readable.cmp(&a.readable).then(a.name.cmp(&b.name)));
        BluetoothSnapshot {
            status: if s.status.is_empty() {
                "idle".into()
            } else {
                s.status.clone()
            },
            error: s.error.clone(),
            devices,
        }
    }
    pub fn available(&self, config: &BluetoothTrigger) -> Result<(), String> {
        config.validate().map_err(|e| e.to_string())?;
        let s = self.lock();
        if s.status != "scanning" {
            return Err(s.error.clone().unwrap_or_else(|| {
                "Open the Bluetooth trigger and scan before enabling this automation".into()
            }));
        }
        let now = now_ms();
        if !s.devices.get(&config.device_id).is_some_and(|d| {
            d.samples.len() >= 3
                && d.reading(now)
                    .is_some_and(|r| now.saturating_sub(r.at_ms) <= BLUETOOTH_STALE_MS)
        }) {
            return Err(
                "The selected Bluetooth device has no fresh signal; bring it nearby and scan again"
                    .into(),
            );
        }
        Ok(())
    }
    pub fn preview(
        &self,
        id: String,
        config: BluetoothTrigger,
    ) -> Result<BluetoothDetection, String> {
        config.validate().map_err(|e| e.to_string())?;
        if id.is_empty() || id.len() > 128 {
            return Err("invalid Bluetooth preview identifier".into());
        }
        let now = now_ms();
        let mut s = self.lock();
        s.preview_until = now + 15_000;
        s.previews
            .retain(|_, p| now.saturating_sub(p.touched) < 30_000);
        if s.previews.len() >= 16 && !s.previews.contains_key(&id) {
            return Err("too many Bluetooth previews".into());
        }
        let reading = s
            .devices
            .get(&config.device_id)
            .and_then(|d| d.reading(now));
        let healthy = s.status == "scanning";
        let p = s.previews.entry(id).or_insert_with(|| Preview {
            config: config.clone(),
            detector: BluetoothDetector::default(),
            touched: now,
        });
        if p.config != config {
            p.config = config.clone();
            p.detector = BluetoothDetector::default();
        }
        p.touched = now;
        Ok(p.detector.update(&config, now, reading, healthy))
    }
    pub async fn calibrate(&self, device_id: String) -> Result<BluetoothSample, String> {
        if device_id.is_empty() || device_id.len() > 512 {
            return Err("select a Bluetooth device".into());
        }
        let start = now_ms();
        let epoch = {
            let mut s = self.lock();
            s.preview_until = start + 30_000;
            s.epoch
        };
        tokio::time::sleep(Duration::from_secs(10)).await;
        let s = self.lock();
        if s.epoch != epoch || s.status != "scanning" {
            return Err("Bluetooth changed during calibration; try again".into());
        }
        let device = s
            .devices
            .get(&device_id)
            .ok_or("No Bluetooth samples received")?;
        let mut samples: Vec<_> = device.samples.iter().filter(|r| r.at_ms >= start).collect();
        // Count distinct one-second buckets; advertisement bursts are not ten seconds of evidence.
        samples.dedup_by_key(|r| r.at_ms / 1000);
        if samples.len() < 6
            || device
                .samples
                .back()
                .is_none_or(|r| now_ms().saturating_sub(r.at_ms) > BLUETOOTH_STALE_MS)
        {
            return Err(
                "Not enough fresh Bluetooth signal; keep the device advertising and try again"
                    .into(),
            );
        }
        let mut values: Vec<_> = samples.iter().map(|r| r.rssi).collect();
        values.sort_unstable();
        if values[values.len() - 2] - values[1] > 20 {
            return Err("Bluetooth signal is unstable; hold the device still and try again".into());
        }
        Ok(BluetoothSample {
            device_id,
            rssi: values[values.len() / 2],
            sample_count: values.len() as u32,
        })
    }
    fn record(&self, id: String, name: Option<String>, rssi: Option<i16>) {
        let now = now_ms();
        let mut s = self.lock();
        s.devices
            .retain(|_, d| now.saturating_sub(d.seen_at) < 600_000);
        if s.devices.len() >= 256 && !s.devices.contains_key(&id) {
            return;
        }
        let device = s.devices.entry(id.clone()).or_default();
        device.seen_at = now;
        if let Some(name) = name.filter(|n| !n.trim().is_empty()) {
            device.name = name.chars().take(128).collect();
        }
        if device.name.is_empty() {
            device.name = id;
        }
        if let Some(rssi) = rssi.filter(|r| (-110..=-20).contains(r)) {
            if device
                .samples
                .back()
                .is_some_and(|r| now.saturating_sub(r.at_ms) < 500)
            {
                return;
            }
            device
                .samples
                .push_back(BluetoothReading { rssi, at_ms: now });
            while device.samples.len() > 120 {
                device.samples.pop_front();
            }
        }
    }
    pub fn start(&self, engine: AutomationEngine) {
        let scanner = self.clone();
        tokio::spawn(async move {
            scanner.scan_loop().await;
        });
        let service = self.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(1));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            let mut monitors: HashMap<String, (u64, u64, BluetoothDetector)> = HashMap::new();
            let mut last_tick = now_ms();
            loop {
                interval.tick().await;
                let now = now_ms();
                if now < last_tick || now.saturating_sub(last_tick) > BLUETOOTH_STALE_MS {
                    service.reset();
                    monitors.clear();
                }
                last_tick = now;
                let definitions = match engine.list().await {
                    Ok(d) => d,
                    Err(e) => {
                        tracing::warn!(%e,"Bluetooth configuration unavailable");
                        continue;
                    }
                };
                let rules: Vec<_> = definitions
                    .into_iter()
                    .filter(|d| {
                        d.enabled && matches!(d.trigger, AutomationTrigger::Bluetooth { .. })
                    })
                    .collect();
                service.lock().enabled = !rules.is_empty();
                monitors.retain(|id, _| rules.iter().any(|d| &d.id == id));
                for d in rules {
                    let AutomationTrigger::Bluetooth { config } = &d.trigger else {
                        continue;
                    };
                    if service
                        .lock()
                        .paused_rules
                        .get(&d.id)
                        .is_some_and(|until| *until > now)
                    {
                        monitors.remove(&d.id);
                        continue;
                    }
                    let (healthy, epoch, reading) = {
                        let s = service.lock();
                        (
                            s.status == "scanning",
                            s.epoch,
                            s.devices
                                .get(&config.device_id)
                                .and_then(|device| device.reading(now)),
                        )
                    };
                    let m = monitors
                        .entry(d.id.clone())
                        .or_insert_with(|| (d.revision, epoch, BluetoothDetector::default()));
                    if m.0 != d.revision || m.1 != epoch {
                        *m = (d.revision, epoch, BluetoothDetector::default());
                    }
                    let detected = m.2.update(config, now, reading, healthy);
                    if detected.fired {
                        let mut event = AutomationEvent::new("bluetooth.far");
                        event.variables.extend([
                            ("event.bluetooth.deviceId".into(), config.device_id.clone()),
                            (
                                "event.bluetooth.deviceName".into(),
                                config.device_name.clone(),
                            ),
                            (
                                "event.bluetooth.rssi".into(),
                                reading.map(|r| r.rssi.to_string()).unwrap_or_default(),
                            ),
                            ("event.bluetooth.reason".into(), detected.state),
                            ("event.bluetooth.ruleId".into(), d.id),
                            ("event.bluetooth.revision".into(), d.revision.to_string()),
                        ]);
                        if let Err(e) = engine.dispatch(event).await {
                            tracing::warn!(%e,"Bluetooth automation dispatch failed");
                        }
                    }
                }
            }
        });
    }
    async fn scan_loop(&self) {
        loop {
            if !self.wanted() {
                self.status("idle", None);
                tokio::time::sleep(Duration::from_secs(1)).await;
                continue;
            }
            self.status("starting", None);
            let result = tokio::time::timeout(Duration::from_secs(12), async {
                let manager = Manager::new().await.map_err(|e| e.to_string())?;
                let adapters = manager.adapters().await.map_err(|e| e.to_string())?;
                for adapter in adapters {
                    if adapter.adapter_state().await.map_err(|e| e.to_string())?
                        == CentralState::PoweredOn
                    {
                        return Ok(adapter);
                    }
                }
                Err("Bluetooth is off or no adapter is available; check system Bluetooth permissions".to_string())
            }).await;
            let adapter = match result {
                Ok(Ok(a)) => a,
                other => {
                    self.status(
                        "unavailable",
                        Some(match other {
                            Ok(Err(e)) => e,
                            _ => "Bluetooth initialization timed out; check system permissions"
                                .into(),
                        }),
                    );
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    continue;
                }
            };
            let run = self.scan_adapter(&adapter).await;
            let _ = tokio::time::timeout(Duration::from_secs(3), adapter.stop_scan()).await;
            if let Err(error) = run {
                self.status("unavailable", Some(error));
                self.reset();
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }
    }
    async fn scan_adapter(&self, adapter: &Adapter) -> Result<(), String> {
        let mut events = adapter.events().await.map_err(|e| e.to_string())?;
        tokio::time::timeout(
            Duration::from_secs(5),
            adapter.start_scan(ScanFilter::default()),
        )
        .await
        .map_err(|_| "Bluetooth scan start timed out")?
        .map_err(|e| e.to_string())?;
        self.reset();
        self.status("scanning", None);
        let mut tick = tokio::time::interval(Duration::from_secs(1));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                _ = tick.tick() => {
                    if !self.wanted() { return Ok(()); }
                    let state = tokio::time::timeout(Duration::from_secs(3), adapter.adapter_state())
                        .await.map_err(|_| "Bluetooth health check timed out")?
                        .map_err(|e| e.to_string())?;
                    if state != CentralState::PoweredOn {
                        return Err("Bluetooth is off; monitoring is paused".into());
                    }
                }
                event = events.next() => {
                    let Some(event) = event else {
                        return Err("Bluetooth event stream ended; reconnecting".into());
                    };
                    let (id, explicit, advertisement) = match event {
                        CentralEvent::StateUpdate(state) if state != CentralState::PoweredOn =>
                            return Err("Bluetooth is unavailable; monitoring is paused".into()),
                        CentralEvent::RssiUpdate { id, rssi } => (id, Some(rssi), true),
                        // Metadata updates may contain cached RSSI. Only radio events
                        // that refresh RSSI in the platform adapter count as new samples.
                        CentralEvent::DeviceDiscovered(id) | CentralEvent::DeviceUpdated(id) =>
                            (id, None, false),
                        CentralEvent::ManufacturerDataAdvertisement { id, .. }
                        | CentralEvent::ServiceDataAdvertisement { id, .. }
                        | CentralEvent::ServicesAdvertisement { id, .. } => (id, None, true),
                        _ => continue,
                    };
                    let props = tokio::time::timeout(Duration::from_secs(2), async {
                        adapter.peripheral(&id).await?.properties().await
                    }).await;
                    let (name, rssi) = match props {
                        Ok(Ok(Some(p))) => (p.local_name, explicit.or(p.rssi)),
                        _ => (None, explicit),
                    };
                    self.record(id.to_string(), name, if advertisement { rssi } else { None });
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn construction_and_snapshot_need_no_tokio_runtime() {
        let service = BluetoothService::default();
        assert_eq!(service.snapshot(None).status, "idle");
        service.record("id".into(), Some("phone".into()), Some(-60));
        assert_eq!(service.snapshot(None).devices.len(), 1);
    }
    #[test]
    fn expired_cached_rssi_keeps_original_timestamp() {
        let mut device = Device::default();
        device.samples.push_back(BluetoothReading {
            rssi: -55,
            at_ms: 1000,
        });
        assert_eq!(device.reading(100000).unwrap().at_ms, 1000);
    }
    #[test]
    fn metadata_does_not_refresh_rssi_or_hide_unreadable_devices() {
        let service = BluetoothService::default();
        service.record("silent".into(), Some("No advertisements".into()), None);
        service.record("phone".into(), Some("Phone".into()), Some(-60));
        let original = service.lock().devices["phone"]
            .samples
            .back()
            .unwrap()
            .at_ms;
        service.record("phone".into(), Some("Renamed phone".into()), None);
        let view = service.snapshot(None);
        assert_eq!(view.devices.len(), 2);
        let silent = view.devices.iter().find(|d| d.id == "silent").unwrap();
        assert!(!silent.readable);
        assert_eq!(silent.rssi, None);
        let state = service.lock();
        assert_eq!(state.devices["phone"].samples.len(), 1);
        assert_eq!(
            state.devices["phone"].samples.back().unwrap().at_ms,
            original
        );
    }
}
