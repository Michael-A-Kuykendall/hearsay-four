import importlib.util
import unittest
from collections import Counter
from pathlib import Path

MODULE_PATH = Path(__file__).with_name("generate_waves.py")
SPEC = importlib.util.spec_from_file_location("generate_waves", MODULE_PATH)
MODULE = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(MODULE)


class GenerateWavesTest(unittest.TestCase):
    def test_plan_balances_existing_counts_to_four(self):
        recorded = (
            MODULE.WORDS[:6]
            + MODULE.WORDS[6:20] * 2
            + MODULE.WORDS[20:] * 3
        )
        wave2, wave3 = MODULE.plan_waves(recorded)

        self.assertEqual(len(wave2), 26)
        self.assertEqual(len(wave3), 30)
        self.assertEqual(set(Counter(recorded + wave2 + wave3).values()), {4})

    def test_additions_are_spaced(self):
        recorded = (
            MODULE.WORDS[:6]
            + MODULE.WORDS[6:20] * 2
            + MODULE.WORDS[20:] * 3
        )
        additions = sum(MODULE.plan_waves(recorded), [])
        last_seen = {}
        for index, word in enumerate(additions):
            if word in last_seen:
                self.assertGreaterEqual(index - last_seen[word], 10)
            last_seen[word] = index


if __name__ == "__main__":
    unittest.main()
