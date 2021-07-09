use instant::{Instant, Duration};

#[derive(Debug, Clone)]
pub struct PacketStats {
    pub packets_tx: usize,
    pub packets_rx: usize,
    pub bytes_tx: usize,
    pub bytes_rx: usize,
    pub last_tx: Instant,
    pub last_rx: Instant,
}

impl Default for PacketStats {
    fn default() -> Self {
        // default the last rx/tx to now.
        // not strictly true in use-udp mode, since we can "connect" without
        // exchanging packets. but always true for use-webrtc.
        let now = Instant::now();
        Self {
            packets_tx: 0,
            packets_rx: 0,
            bytes_tx: 0,
            bytes_rx: 0,
            last_tx: now,
            last_rx: now,
         }
    }
}

impl PacketStats {
    pub fn add_tx(&mut self, num_bytes: usize) {
        self.packets_tx += 1;
        self.bytes_tx += num_bytes;
        self.last_tx = Instant::now();
    }
    pub fn add_rx(&mut self, num_bytes: usize) {
        self.packets_rx += 1;
        self.bytes_rx += num_bytes;
        self.last_rx = Instant::now();
    }
    // returns Duration since last (rx, tx)
    pub fn idle_durations(&self) -> (Duration, Duration) {
        let now = Instant::now();
        let rx = now.duration_since(self.last_rx);
        let tx = now.duration_since(self.last_tx);
        (rx, tx)
    }
}