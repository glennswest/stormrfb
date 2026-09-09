# Conformance inputs

`tools/capture_qemu.py` records two full requests over one ZRLE connection
from a disposable, paused, firmware-only QEMU guest. It separately obtains
the expected RGBA checksum from QMP `screendump`, not from this codec.
No disk, guest network, credentials or existing user session is captured.
Generated metadata records QEMU version, UTC date, dimensions and byte count.
Run capture on Linux, then commit the resulting fixture and metadata.

Synthetic fixtures in Rust tests cover tile subencodings, cursor masks,
Hextile state, CopyRect overlap, message fragmentation and hostile lengths.
TigerVNC/noVNC differential and real installer browser validation remain
separate requirements; QEMU firmware coverage does not establish them.
