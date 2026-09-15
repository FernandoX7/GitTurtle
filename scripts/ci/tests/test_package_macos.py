"""Include the standalone mocked-Apple package fixtures in tooling discovery."""
import importlib.util
from pathlib import Path


def load_tests(loader, _tests, _pattern):
    script = Path(__file__).resolve().parents[2] / "test-package-macos.py"
    spec = importlib.util.spec_from_file_location("macos_package_fixtures", script)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return loader.loadTestsFromModule(module)
