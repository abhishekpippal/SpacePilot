"""Export only the public entitlement verification key for desktop builds."""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parents[1]))

from app.security import entitlement_public_key  # noqa: E402

target = Path(__file__).parents[2] / "src-tauri" / "src" / "entitlement_public_key.txt"
target.write_text(entitlement_public_key(), encoding="ascii")
