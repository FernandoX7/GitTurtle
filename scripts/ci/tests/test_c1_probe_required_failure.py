"""NEVER MERGE: deliberate required-tooling failure for the C1 probe only."""
import os
import unittest


class C1RequiredToolingProbe(unittest.TestCase):
    @unittest.skipUnless(os.environ.get("GITHUB_JOB") == "agent-tooling",
                         "C1 diagnostic targets only the real tooling matrix")
    def test_intentional_required_tooling_failure(self):
        self.fail("INTENTIONAL_C1_REQUIRED_TOOLING_FAILURE: never merge this probe")
