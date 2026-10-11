"""Backwards-compatibility shim for cartilage -> kurogane.
Delegates all execution and imports directly to kurogane.
"""

import sys
from kurogane import *
from kurogane.cli import main

if __name__ == "__main__":
    sys.exit(main())
