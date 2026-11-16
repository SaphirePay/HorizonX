#!/usr/bin/env bash
# The HorizonsX server on the VPS: /opt/horizonsx-server, port 25591, a permanent systemd unit beside the Paper
# SMP (25590), set up as that one is (online mode, survival, seed 12345, 8 of the 10 cores at low priority), with the
# Paper plugins in its plugins folder. Upload first: /tmp/v2pkg/{horizon-client,horizon-server,plugins/*.so}.
set -euo pipefail
D=/opt/horizonsx-server
SMP=/opt/horizon-paper-test/server
mkdir -p $D/runtime $D/engine $D/server/plugins
install -m 755 /tmp/v2pkg/horizon-server $D/runtime/horizon-server
install -m 755 /tmp/v2pkg/horizon-client $D/engine/horizon-client
rm -f $D/server/plugins/*.so
cp /tmp/v2pkg/plugins/*.so $D/server/plugins/
for f in eula.txt ops.json whitelist.json usercache.json banned-players.json banned-ips.json; do
  [ -f "$D/server/$f" ] || cp "$SMP/$f" "$D/server/$f"
done
sed -e 's/^server-port=.*/server-port=25591/' -e 's/^level-name=.*/level-name=world/' -e 's/^max-players=.*/max-players=100/' \
    -e 's/^motd=.*/motd=HorizonsX (Linux)/' "$SMP/server.properties" > $D/server/server.properties
sed -e 's#^motd = .*#motd = "HorizonsX (Linux)"#' -e 's#^port = 25590 #port = 25591 #' -e 's#^name = "parity" #name = "world" #' \
    -e 's#^max_players = 20 #max_players = 100#' \
    -e "s#^path = \"/opt/horizon-paper-test/engine/horizon-client\"#path = \"$D/engine/horizon-client\"#" \
    "$SMP/server.toml" > $D/server/server.toml
grep -E '^(port|name|max_players|path|resources) ' $D/server/server.toml
cat > /etc/systemd/system/horizonsx.service <<EOF
[Unit]
Description=HorizonsX server (Linux, port 25591)
After=network-online.target

[Service]
WorkingDirectory=$D/server
ExecStart=$D/runtime/horizon-server --dir $D/server run
Restart=on-failure
RestartSec=10
# As the Paper SMP: 8 of the 10 cores, at low priority so the other services on the VPS come first.
CPUQuota=800%
Nice=5
MemoryMax=6G
KillSignal=SIGINT
TimeoutStopSec=60

[Install]
WantedBy=multi-user.target
EOF
systemctl daemon-reload
systemctl enable horizonsx >/dev/null 2>&1
ufw allow 25591/tcp comment 'HorizonsX (Linux)' >/dev/null
echo "set up $D"
