#!/usr/bin/env bash
# HorizonSMP + friends onto live HorizonsX (run ON the VPS, files staged in /tmp/smpdeploy). Plugins go in by hot
# swap; nothing here restarts the server. Refuses to run while anyone is online.
#   /tmp/smpdeploy/{libhorizonsmp.so,libluckperms_rs.so,libjobs_rs.so,libmcmmo_rs.so,libzauctionhouse_rs.so,
#                   lp_ranks.txt,command-owners.live.yml}
set -euo pipefail
H=/opt/horizonsx-server
S=$H/server
IN=/tmp/smpdeploy
CMD="$H/runtime/horizon-server --dir $S cmd"
on=$(ss -tn state established "( sport = :25591 )" | tail -n +2 | wc -l)
if [ "$on" != "0" ]; then echo "players online ($on): not deploying"; exit 1; fi
stamp=$(date +%Y%m%d-%H%M)
mkdir -p $H/plugins-backup-$stamp
cp -a $S/plugins/libluckperms_rs.so $H/plugins-backup-$stamp/ 2>/dev/null || true
cp -a $S/plugins/LuckPerms $H/plugins-backup-$stamp/LuckPerms
cp -a $S/plugins/Essentials/config.yml $H/plugins-backup-$stamp/essentials-config.yml
echo "backup in $H/plugins-backup-$stamp"

# Essentials: HorizonSMP does the first-join welcome and kit.
sed -i -e "s/^  announce-format: '&dWelcome {DISPLAYNAME}&d to the server!'/  announce-format: ''/" -e "s/^  kit: tools\s*$/  kit: ''/" $S/plugins/Essentials/config.yml

# Who answers shared commands (read by an engine with command owners; harmless for older ones).
cp $IN/command-owners.live.yml $S/command-owners.yml

# Plugins: written beside, then moved in, so the hot swap never reads half a file.
for f in libluckperms_rs.so libhorizonsmp.so libjobs_rs.so libmcmmo_rs.so libzauctionhouse_rs.so; do
  cp $IN/$f $S/plugins/.$f.new && mv $S/plugins/.$f.new $S/plugins/$f
  echo "in: $f"
  sleep 4
done
sleep 6
grep -a -E "HotReload\] (swapped|failed)" $S/logs/latest.log | tail -6

# LuckPerms ranks (idempotent: sets nodes, re-adds parents, replaces prefixes).
for g in vip helper mod admin owner; do $CMD lp group $g meta removeprefix >/dev/null 2>&1 || true; done
grep -v -E '^\s*(#|$)' $IN/lp_ranks.txt | while IFS= read -r line; do $CMD lp $line >/dev/null 2>&1 || echo "failed: lp $line"; done
echo "ranks applied"
$CMD essentials reload >/dev/null 2>&1 || true
$CMD commands reload >/dev/null 2>&1 || true
echo "deploy done"
