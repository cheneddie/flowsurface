from pathlib import Path

path = Path("data/src/lib.rs")
text = path.read_text(encoding="utf-8")

if "pub mod market_gateway;" not in text:
    text = text.replace("pub mod market;\n", "pub mod market;\npub mod market_gateway;\n", 1)
if "pub mod replay_session;" not in text:
    text = text.replace("pub mod replay;\n", "pub mod replay;\npub mod replay_session;\n", 1)

for needle in ("pub mod market_gateway;", "pub mod replay_session;"):
    if needle not in text:
        raise SystemExit(f"module wiring failed: {needle}")

path.write_text(text, encoding="utf-8")
