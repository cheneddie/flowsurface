from pathlib import Path

path = Path("data/src/orderflow/advanced.rs")
text = path.read_text(encoding="utf-8")

old = """    if buy_qty >= config.min_aggressive_qty
        && upward_ticks <= u64::from(config.max_price_move_ticks)
    {"""
new = """    if !buy_qty.is_zero()
        && buy_qty >= config.min_aggressive_qty
        && upward_ticks <= u64::from(config.max_price_move_ticks)
    {"""
if old in text:
    text = text.replace(old, new, 1)

old = """    if sell_qty >= config.min_aggressive_qty
        && downward_ticks <= u64::from(config.max_price_move_ticks)
    {"""
new = """    if !sell_qty.is_zero()
        && sell_qty >= config.min_aggressive_qty
        && downward_ticks <= u64::from(config.max_price_move_ticks)
    {"""
if old in text:
    text = text.replace(old, new, 1)

marker = """    #[test]
    fn speed_collapse_flags_exhaustion() {"""
test = """    #[test]
    fn one_sided_flow_does_not_emit_zero_quantity_opposite_absorption() {
        let trades = [trade(1, false, 100.0, 10.0)];
        let signals = detect_absorption(
            &trades,
            p(100.0),
            p(100.0),
            step(0.5),
            AbsorptionConfig::default(),
        );

        assert!(signals.iter().any(|signal| signal.aggressive_side == AggressiveSide::Buy));
        assert!(!signals.iter().any(|signal| signal.aggressive_side == AggressiveSide::Sell));
    }

    #[test]
    fn speed_collapse_flags_exhaustion() {"""
if marker in text and "one_sided_flow_does_not_emit_zero_quantity_opposite_absorption" not in text:
    text = text.replace(marker, test, 1)

if "if !buy_qty.is_zero()" not in text or "if !sell_qty.is_zero()" not in text:
    raise SystemExit("absorption zero-flow hardening missing")

path.write_text(text, encoding="utf-8")
