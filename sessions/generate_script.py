"""Legacy entry point preserved for compatibility.

The 160-line unconstrained shuffle was superseded by the wave plan in
``generate_waves.py``, which starts from the recorded run1 manifest and
brings every closed-vocabulary word to exactly four exposures.
"""
from generate_waves import main

if __name__ == "__main__":
    main()
