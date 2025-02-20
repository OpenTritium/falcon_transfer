struct LinkState {
    // 原有字段
    ewma_metric: AtomicU64, // 动态调整的指标
    last_update: AtomicU64, // 最后更新时间戳
}

impl LinkState {
    // 每次使用后更新EWMA
    fn update_ewma(&self, new_metric: u64) {
        let now = current_timestamp();
        let last = self.last_update.load(Ordering::Relaxed);
        let time_diff = now.saturating_sub(last) as f64;
        
        // α = 1 - e^(-time_diff/τ), τ为时间常数（如60秒）
        let alpha = 1.0 - (-time_diff / 60.0).exp();
        let old = self.ewma_metric.load(Ordering::Relaxed) as f64;
        let new = old * (1.0 - alpha) + new_metric as f64 * alpha;
        
        self.ewma_metric.store(new as u64, Ordering::Relaxed);
        self.last_update.store(now, Ordering::Relaxed);
    }
}