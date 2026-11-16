#!/usr/bin/env python3
"""HorizonJobs' test rig: EssentialsX (economy) + LuckPerms-rs + HorizonSMP + HorizonJobs on a scratch server
(port 25797, parity/jobs) with the offscreen client "Prover", on the HorizonsX Windows engine. Uses HorizonSMP's
rig (smp_live.py) with its own folder and plugin list.

    jobs_rig.py up | deploy [NAME] | say LINE... | shot NAME [LINE...] | log [N] | relaunch | down
"""
import os
import pathlib
import sys

sys.path.insert(0, r'D:\Horizon\plugin-ports\horizonsmp_rs\tests')
os.environ.setdefault('RW_ENGINE', r'D:\Horizon\perf\hxcmdowners\horizon-client.exe')
import smp_live  # noqa: E402

B = pathlib.Path(r'D:\Horizon\hx-plugin-build')
smp_live.RUN = pathlib.Path(r'D:\Horizon\parity\jobs')
smp_live.PORT = 25797
smp_live.SERVER = smp_live.RUN / 'server'
smp_live.LOG = smp_live.SERVER / 'logs' / 'latest.log'
smp_live.CLIENT_LOG = smp_live.RUN / 'client' / 'logs' / 'latest.log'
smp_live.DLLS = [
    B / 't_ess' / 'release' / 'essentials_bundle.dll',
    B / 't_rw' / 'release' / 'luckperms_rs.dll',
    B / 't_rw' / 'release' / 'horizonsmp.dll',
    B / 't_rw' / 'release' / 'horizonjobs.dll',
]
smp_live.proof.RUN = smp_live.RUN
smp_live.proof.PORT = smp_live.PORT

if __name__ == '__main__':
    smp_live.main()
