"""Where the plugin tools work: the workspace folder holding the engine checkouts, the plugin corpus, the ports and
the test servers (`HORIZON_WORKSPACE`, by default `~/Horizon`), and the Java they run (`HORIZON_JAVA`, else `java`
on the PATH)."""
import os
import pathlib
import shutil

WORKSPACE = pathlib.Path(os.environ.get('HORIZON_WORKSPACE') or pathlib.Path.home() / 'Horizon')
JAVA = os.environ.get('HORIZON_JAVA') or shutil.which('java') or 'java'


def at(*parts):
    """A path inside the workspace."""
    return WORKSPACE.joinpath(*parts)
