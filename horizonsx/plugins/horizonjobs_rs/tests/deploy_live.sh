#!/usr/bin/env bash
# HorizonJobs (+ HorizonSMP with its /menu Jobs button) onto live HorizonsX. Run ON the VPS with the files in
# /tmp/jobsdeploy: libhorizonjobs.so, libhorizonsmp.so. Hot swap only, no restart. Refuses while anyone is online.
set -euo pipefail
H=/opt/horizonsx-server
S=$H/server
IN=/tmp/jobsdeploy
on=$(ss -tn state established "( sport = :25591 )" | tail -n +2 | wc -l)
if [ "$on" != "0" ] && [ "${FORCE:-}" != "1" ]; then echo "players online ($on): not deploying"; exit 1; fi
stamp=$(date +%Y%m%d-%H%M)
mkdir -p $H/plugins-backup-$stamp
cp -a $S/plugins/libhorizonsmp.so $H/plugins-backup-$stamp/
[ -d $S/plugins/HorizonJobs ] && cp -a $S/plugins/HorizonJobs $H/plugins-backup-$stamp/HorizonJobs
echo "backup in $H/plugins-backup-$stamp"
# The old Jobs plugin stays off (it also answers /jobs).
[ -f $S/plugins/libjobs_rs.so ] && mv $S/plugins/libjobs_rs.so $S/plugins/libjobs_rs.so.disabled
for f in libhorizonjobs.so libhorizonsmp.so; do
  cp $IN/$f $S/plugins/.$f.new && mv $S/plugins/.$f.new $S/plugins/$f
  echo "in: $f"
  sleep 5
done
sleep 5
grep -a -E "HotReload\] (swapped|failed|loaded)|HorizonJobs" $S/logs/latest.log | tail -8
echo "deploy done"
