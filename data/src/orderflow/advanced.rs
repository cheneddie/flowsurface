use std::collections::BTreeSet;

use exchange::{
    Trade, UnixMs,
    depth::Depth,
    unit::{Price, PriceStep, Qty},
};

use super::{BookSide, TradeSpeedSnapshot};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AggressiveSide {
    Buy,
    Sell,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LargeTradeSignal {
    pub time: UnixMs,
    pub side: AggressiveSide,
    pub price: Price,
    pub qty: Qty,
}

pub fn detect_large_trade(trade: &Trade, min_qty: Qty) -> Option<LargeTradeSignal> {
    (trade.qty >= min_qty).then_some(LargeTradeSignal {
        time: trade.time,
        side: if trade.is_sell {
            AggressiveSide::Sell
        } else {
            AggressiveSide::Buy
        },
        price: trade.price,
        qty: trade.qty,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiquidityChangeKind {
    Stack,
    /// A material depth reduction that was not listed as confirmed execution.
    /// It remains a candidate because aggregate L2 alone cannot prove intent.
    PullCandidate,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LiquidityChangeSignal {
    pub side: BookSide,
    pub price: Price,
    pub before: Qty,
    pub after: Qty,
    pub kind: LiquidityChangeKind,
    /// Relative absolute change versus the previous displayed quantity.
    pub change_ratio: f64,
}

/// Detect material stacking / pull candidates from two depth states.
///
/// `confirmed_consumed_prices` lets a caller exclude reductions already
/// attributed to executions by the BookConsumptionDetector.
pub fn detect_liquidity_changes(
    previous: &Depth,
    current: &Depth,
    side: BookSide,
    min_change_ratio: f64,
    min_before_qty: Qty,
    confirmed_consumed_prices: &BTreeSet<Price>,
) -> Vec<LiquidityChangeSignal> {
    let min_change_ratio = min_change_ratio.clamp(0.0, 1.0);
    let (old_levels, new_levels) = match side {
        BookSide::Bid => (&previous.bids, &current.bids),
        BookSide::Ask => (&previous.asks, &current.asks),
    };

    let mut prices = BTreeSet::new();
    prices.extend(old_levels.keys().copied());
    prices.extend(new_levels.keys().copied());

    let mut out = Vec::new();
    for price in prices {
        let before = old_levels.get(&price).copied().unwrap_or(Qty::ZERO);
        let after = new_levels.get(&price).copied().unwrap_or(Qty::ZERO);
        if before == after {
            continue;
        }

        let reference = before.max(after);
        if reference < min_before_qty || reference.is_zero() {
            continue;
        }

        let change = before.abs_diff(after);
        let ratio = change.to_f64() / reference.to_scale_or_one();
        if ratio + f64::EPSILON < min_change_ratio {
            continue;
        }

        let kind = if after > before {
            LiquidityChangeKind::Stack
        } else {
            if confirmed_consumed_prices.contains(&price) {
                continue;
            }
            LiquidityChangeKind::PullCandidate
        };

        out.push(LiquidityChangeSignal {
            side,
            price,
            before,
            after,
            kind,
            change_ratio: ratio,
        });
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AbsorptionConfig {
    pub min_aggressive_qty: Qty,
    pub max_price_move_ticks: u32,
}

impl Default for AbsorptionConfig {
    fn default() -> Self {
        Self {
            min_aggressive_qty: Qty::ZERO,
            max_price_move_ticks: 1,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AbsorptionSignal {
    /// Side initiating the aggressive flow that failed to move price materially.
    pub aggressive_side: AggressiveSide,
    /// Resting side inferred to have absorbed the pressure.
    pub resting_side: BookSide,
    pub aggressive_qty: Qty,
    pub start_price: Price,
    pub end_price: Price,
    pub move_ticks: u64,
}

pub fn detect_absorption(
    trades: &[Trade],
    start_price: Price,
    end_price: Price,
    step: PriceStep,
    config: AbsorptionConfig,
) -> Vec<AbsorptionSignal> {
    if trades.is_empty() || step.units == 0 {
        return Vec::new();
    }

    let (buy_qty, sell_qty) = trades
        .iter()
        .fold((Qty::ZERO, Qty::ZERO), |(buy, sell), trade| {
            if trade.is_sell {
                (buy, sell + trade.qty)
            } else {
                (buy + trade.qty, sell)
            }
        });

    let upward_ticks = directional_ticks(start_price, end_price, step, true);
    let downward_ticks = directional_ticks(start_price, end_price, step, false);
    let mut out = Vec::new();

    if buy_qty >= config.min_aggressive_qty
        && upward_ticks <= u64::from(config.max_price_move_ticks)
    {
        out.push(AbsorptionSignal {
            aggressive_side: AggressiveSide::Buy,
            resting_side: BookSide::Ask,
            aggressive_qty: buy_qty,
            start_price,
            end_price,
            move_ticks: upward_ticks,
        });
    }

    if sell_qty >= config.min_aggressive_qty
        && downward_ticks <= u64::from(config.max_price_move_ticks)
    {
        out.push(AbsorptionSignal {
            aggressive_side: AggressiveSide::Sell,
            resting_side: BookSide::Bid,
            aggressive_qty: sell_qty,
            start_price,
            end_price,
            move_ticks: downward_ticks,
        });
    }

    out
}

fn directional_ticks(start: Price, end: Price, step: PriceStep, upward: bool) -> u64 {
    let step_units = step.units.unsigned_abs().max(1);
    let movement = if upward {
        end.units.saturating_sub(start.units).max(0) as u64
    } else {
        start.units.saturating_sub(end.units).max(0) as u64
    };
    movement / step_units
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExhaustionConfig {
    pub min_previous_volume_rate: f64,
    /// Current / previous volume-speed ratio must be <= this value.
    pub max_remaining_ratio: f64,
}

impl Default for ExhaustionConfig {
    fn default() -> Self {
        Self {
            min_previous_volume_rate: 0.0,
            max_remaining_ratio: 0.35,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExhaustionSignal {
    pub previous_volume_rate: f64,
    pub current_volume_rate: f64,
    pub previous_side: Option<AggressiveSide>,
    pub remaining_ratio: f64,
}

pub fn detect_exhaustion(
    previous: TradeSpeedSnapshot,
    current: TradeSpeedSnapshot,
    config: ExhaustionConfig,
) -> Option<ExhaustionSignal> {
    if previous.volume_rate < config.min_previous_volume_rate
        || previous.volume_rate <= f64::EPSILON
    {
        return None;
    }

    let remaining_ratio = current.volume_rate / previous.volume_rate;
    if remaining_ratio > config.max_remaining_ratio.max(0.0) {
        return None;
    }

    let previous_side = if previous.delta_volume_rate > 0.0 {
        Some(AggressiveSide::Buy)
    } else if previous.delta_volume_rate < 0.0 {
        Some(AggressiveSide::Sell)
    } else {
        None
    };

    Some(ExhaustionSignal {
        previous_volume_rate: previous.volume_rate,
        current_volume_rate: current.volume_rate,
        previous_side,
        remaining_ratio,
    })
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StopRunConfig {
    pub min_swept_levels: u32,
    pub min_sweep_qty: Qty,
    pub min_reversal_ticks: u32,
    pub max_reversal_ms: u64,
}

impl Default for StopRunConfig {
    fn default() -> Self {
        Self {
            min_swept_levels: 2,
            min_sweep_qty: Qty::ZERO,
            min_reversal_ticks: 2,
            max_reversal_ms: 2_000,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StopRunObservation {
    pub swept_side: BookSide,
    pub swept_levels: u32,
    pub sweep_qty: Qty,
    pub extreme_price: Price,
    pub post_price: Price,
    pub reversal_elapsed_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StopRunSignal {
    pub swept_side: BookSide,
    pub swept_levels: u32,
    pub sweep_qty: Qty,
    pub reversal_ticks: u64,
    pub reversal_elapsed_ms: u64,
}

pub fn detect_stop_run(
    observation: StopRunObservation,
    step: PriceStep,
    config: StopRunConfig,
) -> Option<StopRunSignal> {
    if observation.swept_levels < config.min_swept_levels
        || observation.sweep_qty < config.min_sweep_qty
        || observation.reversal_elapsed_ms > config.max_reversal_ms
        || step.units == 0
    {
        return None;
    }

    let reversal_ticks = match observation.swept_side {
        // Ask-side sweep is an upward run; confirmation requires reversal down.
        BookSide::Ask => directional_ticks(
            observation.post_price,
            observation.extreme_price,
            step,
            true,
        ),
        // Bid-side sweep is a downward run; confirmation requires reversal up.
        BookSide::Bid => directional_ticks(
            observation.extreme_price,
            observation.post_price,
            step,
            true,
        ),
    };

    if reversal_ticks < u64::from(config.min_reversal_ticks) {
        return None;
    }

    Some(StopRunSignal {
        swept_side: observation.swept_side,
        swept_levels: observation.swept_levels,
        sweep_qty: observation.sweep_qty,
        reversal_ticks,
        reversal_elapsed_ms: observation.reversal_elapsed_ms,
    })
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LargeRestingOrder {
    pub side: BookSide,
    pub price: Price,
    pub qty: Qty,
}

pub fn detect_large_resting_orders(depth: &Depth, min_qty: Qty) -> Vec<LargeRestingOrder> {
    let bids = depth.bids.iter().filter_map(|(&price, &qty)| {
        (qty >= min_qty).then_some(LargeRestingOrder {
            side: BookSide::Bid,
            price,
            qty,
        })
    });
    let asks = depth.asks.iter().filter_map(|(&price, &qty)| {
        (qty >= min_qty).then_some(LargeRestingOrder {
            side: BookSide::Ask,
            price,
            qty,
        })
    });
    bids.chain(asks).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(value: f64) -> Price {
        Price::from_f64(value)
    }

    fn q(value: f64) -> Qty {
        Qty::from_f64(value)
    }

    fn step(value: f64) -> PriceStep {
        PriceStep {
            units: p(value).units,
        }
    }

    fn trade(ms: u64, is_sell: bool, price: f64, qty: f64) -> Trade {
        Trade {
            time: UnixMs::new(ms),
            is_sell,
            price: p(price),
            qty: q(qty),
        }
    }

    #[test]
    fn large_trade_threshold_is_deterministic() {
        assert!(detect_large_trade(&trade(1, false, 100.0, 10.0), q(10.0)).is_some());
        assert!(detect_large_trade(&trade(1, false, 100.0, 9.9), q(10.0)).is_none());
    }

    #[test]
    fn confirmed_execution_is_not_mislabeled_as_pull() {
        let mut before = Depth::default();
        before.asks.insert(p(101.0), q(10.0));
        let mut after = Depth::default();
        after.asks.insert(p(101.0), q(2.0));

        let mut consumed = BTreeSet::new();
        consumed.insert(p(101.0));
        let signals =
            detect_liquidity_changes(&before, &after, BookSide::Ask, 0.5, q(1.0), &consumed);
        assert!(signals.is_empty());
    }

    #[test]
    fn material_new_liquidity_is_stack() {
        let before = Depth::default();
        let mut after = Depth::default();
        after.bids.insert(p(99.0), q(10.0));

        let signals = detect_liquidity_changes(
            &before,
            &after,
            BookSide::Bid,
            0.5,
            q(5.0),
            &BTreeSet::new(),
        );
        assert_eq!(signals.len(), 1);
        assert_eq!(signals[0].kind, LiquidityChangeKind::Stack);
    }

    #[test]
    fn heavy_buying_without_upward_progress_flags_ask_absorption() {
        let trades = [trade(1, false, 100.0, 6.0), trade(2, false, 100.0, 5.0)];
        let signals = detect_absorption(
            &trades,
            p(100.0),
            p(100.5),
            step(0.5),
            AbsorptionConfig {
                min_aggressive_qty: q(10.0),
                max_price_move_ticks: 1,
            },
        );

        assert!(signals.iter().any(|signal| {
            signal.aggressive_side == AggressiveSide::Buy && signal.resting_side == BookSide::Ask
        }));
    }

    #[test]
    fn speed_collapse_flags_exhaustion() {
        let previous = TradeSpeedSnapshot {
            volume_rate: 100.0,
            delta_volume_rate: 30.0,
            ..TradeSpeedSnapshot::default()
        };
        let current = TradeSpeedSnapshot {
            volume_rate: 20.0,
            ..TradeSpeedSnapshot::default()
        };

        let signal = detect_exhaustion(
            previous,
            current,
            ExhaustionConfig {
                min_previous_volume_rate: 50.0,
                max_remaining_ratio: 0.3,
            },
        )
        .expect("exhaustion");

        assert_eq!(signal.previous_side, Some(AggressiveSide::Buy));
        assert!((signal.remaining_ratio - 0.2).abs() < f64::EPSILON);
    }

    #[test]
    fn multi_level_sweep_with_fast_reversal_flags_stop_run() {
        let signal = detect_stop_run(
            StopRunObservation {
                swept_side: BookSide::Ask,
                swept_levels: 4,
                sweep_qty: q(20.0),
                extreme_price: p(102.0),
                post_price: p(100.5),
                reversal_elapsed_ms: 500,
            },
            step(0.5),
            StopRunConfig {
                min_swept_levels: 3,
                min_sweep_qty: q(10.0),
                min_reversal_ticks: 2,
                max_reversal_ms: 1_000,
            },
        )
        .expect("stop run");

        assert_eq!(signal.reversal_ticks, 3);
    }

    #[test]
    fn large_resting_orders_scan_both_sides() {
        let mut depth = Depth::default();
        depth.bids.insert(p(99.0), q(12.0));
        depth.asks.insert(p(101.0), q(15.0));
        depth.asks.insert(p(102.0), q(2.0));

        let signals = detect_large_resting_orders(&depth, q(10.0));
        assert_eq!(signals.len(), 2);
    }
}
