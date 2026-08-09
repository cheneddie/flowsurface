use std::{collections::BTreeMap, ops::RangeInclusive};

use iced::widget::{center, text};

use crate::chart::{
    Basis, Caches, Message, ViewState,
    indicator::{
        indicator_row,
        kline::{AvailabilityCause, IndicatorAvailability, KlineIndicatorImpl},
        plot::{AnySeries, PlotTooltip, line::LinePlot},
    },
};

use data::{
    chart::{PlotData, kline::KlineDataPoint},
    orderflow::{SpeedConfig, TradeSpeedEngine, TradeSpeedSnapshot},
    util::format_with_commas,
};
use exchange::{Timeframe, Trade, UnixMs};

const DEFAULT_WINDOW_MS: u64 = 1_000;

#[derive(Debug, Clone, Copy)]
struct TradeSpeedPoint {
    snapshot: TradeSpeedSnapshot,
}

pub struct TradeSpeedIndicator {
    cache: Caches,
    engine: TradeSpeedEngine,
    data: BTreeMap<UnixMs, TradeSpeedPoint>,
    interval: Option<Timeframe>,
    availability: IndicatorAvailability,
}

impl TradeSpeedIndicator {
    pub fn new() -> Self {
        Self {
            cache: Caches::default(),
            engine: TradeSpeedEngine::new(SpeedConfig::new(DEFAULT_WINDOW_MS)),
            data: BTreeMap::new(),
            interval: None,
            availability: IndicatorAvailability::Unknown,
        }
    }

    fn indicator_elem<'a>(
        &'a self,
        main_chart: &'a ViewState,
        data_labels_always_visible: bool,
        visible_range: RangeInclusive<u64>,
    ) -> iced::Element<'a, Message> {
        if let Some(message) = self.unavailable_message(main_chart, "Trade Speed") {
            return center(text(message)).into();
        }

        let value_fn = |point: &TradeSpeedPoint| point.snapshot.delta_volume_rate as f32;
        let tooltip = |point: &TradeSpeedPoint, _next: Option<&TradeSpeedPoint>| {
            PlotTooltip::new(Self::tooltip_text(point.snapshot))
        };

        let plot = LinePlot::new(value_fn)
            .stroke_width(1.2)
            .show_points(true)
            .point_radius_factor(0.18)
            .padding(0.10)
            .with_tooltip(tooltip);

        indicator_row(
            main_chart,
            &self.cache,
            data_labels_always_visible,
            plot,
            AnySeries::forward_unix_ms(&self.data),
            visible_range,
        )
    }

    fn signed_number(value: f64) -> String {
        if value > 0.0 {
            format!("+{}", format_with_commas(value))
        } else {
            format_with_commas(value)
        }
    }

    fn tooltip_text(snapshot: TradeSpeedSnapshot) -> String {
        let accel_pct = snapshot
            .volume_acceleration_pct
            .map(|v| format!("{v:+.1}%"))
            .unwrap_or_else(|| "n/a".to_owned());

        format!(
            "Trade Speed ({} ms)\nTrades/s: {}  Buy: {}  Sell: {}\nVolume/s: {}\nBuy Vol/s: {}  Sell Vol/s: {}\nDelta Vol/s: {}\nVolume Accel: {:+.2}/s² ({})\nDelta Share: {:+.1}%",
            snapshot.window_ms,
            format_with_commas(snapshot.trade_rate),
            format_with_commas(snapshot.buy_trade_rate),
            format_with_commas(snapshot.sell_trade_rate),
            format_with_commas(snapshot.volume_rate),
            format_with_commas(snapshot.buy_volume_rate),
            format_with_commas(snapshot.sell_volume_rate),
            Self::signed_number(snapshot.delta_volume_rate),
            snapshot.volume_acceleration,
            accel_pct,
            snapshot.delta_share * 100.0,
        )
    }

    fn ingest_trades(&mut self, trades: &[Trade]) {
        let Some(interval) = self.interval else {
            return;
        };

        if trades.is_empty() {
            return;
        }

        // Historical fetches may arrive in batches and should not rely on the
        // transport preserving strict chronological ordering.
        let mut ordered = trades.to_vec();
        ordered.sort_by_key(|trade| trade.time);

        let mut touched = false;
        for trade in &ordered {
            if !self.engine.push_trade(trade) {
                continue;
            }

            if let Some(snapshot) = self.engine.snapshot() {
                let bucket = trade.time.floor_to(interval);
                self.data.insert(bucket, TradeSpeedPoint { snapshot });
                touched = true;
            }
        }

        if touched {
            self.availability = IndicatorAvailability::Available;
            self.clear_all_caches();
        }
    }

    fn reset_for_source(&mut self, source: &PlotData<KlineDataPoint>) {
        self.engine.clear();
        self.data.clear();

        match source {
            PlotData::TimeBased(timeseries) => {
                self.interval = Some(timeseries.interval);
                self.availability = IndicatorAvailability::Unknown;
            }
            PlotData::TickBased(_) => {
                self.interval = None;
                // The concrete Tick basis is supplied by `availability()` from
                // the chart state. Keeping this neutral avoids fabricating a
                // TickCount value here.
                self.availability = IndicatorAvailability::Unknown;
            }
        }

        self.clear_all_caches();
    }
}

impl Default for TradeSpeedIndicator {
    fn default() -> Self {
        Self::new()
    }
}

impl KlineIndicatorImpl for TradeSpeedIndicator {
    fn clear_all_caches(&mut self) {
        self.cache.clear_all();
    }

    fn clear_crosshair_caches(&mut self) {
        self.cache.clear_crosshair();
    }

    fn element<'a>(
        &'a self,
        chart: &'a ViewState,
        data_labels_always_visible: bool,
        visible_range: RangeInclusive<u64>,
    ) -> iced::Element<'a, Message> {
        self.indicator_elem(chart, data_labels_always_visible, visible_range)
    }

    fn availability(&self, chart: &ViewState) -> IndicatorAvailability {
        match chart.basis {
            Basis::Tick(_) => {
                IndicatorAvailability::Unavailable(AvailabilityCause::Basis(chart.basis))
            }
            Basis::Time(_) => self.availability.clone(),
        }
    }

    fn rebuild_from_source(&mut self, source: &PlotData<KlineDataPoint>) {
        self.reset_for_source(source);
    }

    fn seed_trades(&mut self, trades: &[Trade], source: &PlotData<KlineDataPoint>) {
        self.reset_for_source(source);
        self.ingest_trades(trades);
    }

    fn on_insert_trades(
        &mut self,
        trades: &[Trade],
        _old_dp_len: usize,
        _source: &PlotData<KlineDataPoint>,
    ) {
        self.ingest_trades(trades);
    }

    fn on_basis_change(&mut self, source: &PlotData<KlineDataPoint>) {
        self.reset_for_source(source);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tooltip_contains_core_speed_metrics() {
        let snapshot = TradeSpeedSnapshot {
            timestamp: UnixMs::new(1_000),
            window_ms: 1_000,
            trade_rate: 10.0,
            buy_trade_rate: 6.0,
            sell_trade_rate: 4.0,
            volume_rate: 25.0,
            buy_volume_rate: 15.0,
            sell_volume_rate: 10.0,
            delta_volume_rate: 5.0,
            trade_acceleration: 1.0,
            volume_acceleration: 2.0,
            buy_volume_acceleration: 1.5,
            sell_volume_acceleration: 0.5,
            trade_acceleration_pct: Some(10.0),
            volume_acceleration_pct: Some(20.0),
            buy_volume_acceleration_pct: Some(15.0),
            sell_volume_acceleration_pct: Some(5.0),
            delta_share: 0.2,
        };

        let tooltip = TradeSpeedIndicator::tooltip_text(snapshot);
        assert!(tooltip.contains("Trades/s"));
        assert!(tooltip.contains("Buy Vol/s"));
        assert!(tooltip.contains("Sell Vol/s"));
        assert!(tooltip.contains("Delta Vol/s: +"));
        assert!(tooltip.contains("Volume Accel"));
        assert!(tooltip.contains("Delta Share"));
    }

    #[test]
    fn signed_number_adds_plus_only_to_positive_values() {
        assert!(TradeSpeedIndicator::signed_number(2.0).starts_with('+'));
        assert!(TradeSpeedIndicator::signed_number(-2.0).starts_with('-'));
        assert_eq!(TradeSpeedIndicator::signed_number(0.0), "0");
    }
}
