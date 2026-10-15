#!/bin/bash
# Installs the body double app on the Garden control guest (VM140)
# (deploy/body-double.md). Idempotent. Run as root from a checkout, after
# `cargo build --release --locked --offline -p body-double-app --bin greenhouse-body-double`:
#
#     deploy/body-double/install-body-double.sh <path to greenhouse-body-double binary>
#
# Registration then creates a pending contract; nothing is authorized until
# the owner approves it.
set -euo pipefail
umask 022
here="$(cd "$(dirname "$0")" && pwd)"
deploy="$(dirname "$here")"
binary="${1:?path to the built greenhouse-body-double binary}"
stamp="$(date -u +%Y%m%dT%H%M%SZ)"
egress_file=/etc/nftables/greenhouse-production-egress.nft
app_etc=/etc/greenhouse/apps/body-double

echo "== principal and files"
systemd-sysusers "$deploy/greenhouse.sysusers"
# The user's line is in deploy/greenhouse.sysusers (deploy/body-double.md).
getent passwd greenhouse-body-double >/dev/null || {
  echo "no greenhouse-body-double user: add its line to deploy/greenhouse.sysusers" >&2
  exit 1
}
install -m 0555 "$binary" /opt/greenhouse/bin/greenhouse-body-double
# SELinux: give the new binary its proper label (VM140 enforces).
if command -v restorecon >/dev/null; then
  restorecon -F /opt/greenhouse/bin/greenhouse-body-double
fi
install -d -m 0750 -o root -g greenhouse-body-double "$app_etc"
[ -f "$app_etc/config.json" ] || install -m 0640 -o root -g greenhouse-body-double "$here/config.example.json" "$app_etc/config.json"

echo "== egress rules"
# The file: sets this table lacks, then the rules, before the chain's end.
if ! grep -q body_double_cgroups "$egress_file"; then
  cp -p "$egress_file" "$egress_file.pre-body-double-$stamp"
  python3 - "$here/egress.nft" "$egress_file" <<'PY'
import re, sys
spec, target = open(sys.argv[1]).read(), sys.argv[2]
text = open(target).read()
block = "\n".join(l[6:] for l in spec.splitlines() if l.startswith("#     "))
sets = []
for name in re.findall(r"^set (\w+) \{", block, re.M):
    if f"set {name} {{" in text:
        continue
    start = block.index(f"set {name} {{")
    end = block.index("\n}", start) + 2
    sets.append("\n".join("    " + l for l in block[start:end].splitlines()))
assert text.count("    chain output {") == 1, "unexpected egress file layout"
if sets:
    text = text.replace("    chain output {", "\n".join(sets) + "\n    chain output {")
rules = [l for l in spec.splitlines() if l.startswith("socket ")]
end = text.rindex("\n    }\n}")
text = text[:end] + "".join("\n        " + r for r in rules) + text[end:]
open(target, "w").write(text)
PY
  nft -c -f /etc/sysconfig/nftables.conf
fi
# The running ruleset gets the same with `nft add`: reloading the file would
# empty the cgroup sets systemd filled for the running services.
python3 - "$here/egress.nft" <<'PY'
import re, subprocess, sys
spec = open(sys.argv[1]).read()
def nft(*args, check=True):
    return subprocess.run(["nft", *args], capture_output=True, text=True, check=check)
block = "\n".join(l[6:] for l in spec.splitlines() if l.startswith("#     "))
for name in re.findall(r"^set (\w+) \{", block, re.M):
    if nft("list", "set", "inet", "greenhouse_egress", name, check=False).returncode != 0:
        start = block.index(f"set {name} {{")
        body = block[block.index("{", start) + 1: block.index("\n}", start)]
        body = "; ".join(l.strip() for l in body.strip().splitlines()).replace(",;", ",")
        nft("add", "set", "inet", "greenhouse_egress", name, "{ " + body + "; }")
        print("added set", name)
if "body_double_cgroups" not in nft("list", "chain", "inet", "greenhouse_egress", "output").stdout:
    # One transaction: all three rules or none, so a half-added set never
    # leaves the app without its final drop (a re-run would skip it).
    rules = [l for l in spec.splitlines() if l.startswith("socket ")]
    subprocess.run(["nft", "-f", "-"], check=True, text=True,
                   input="".join(f"add rule inet greenhouse_egress output {r}\n" for r in rules))
    print("added rules")
PY

echo "== app slot"
if [ ! -f "$app_etc/registration.credential" ]; then
  /opt/greenhouse/bin/greenhouse-admin provision-app --db /var/lib/greenhouse/kernel/kernel.db \
    --app-id body-double --endpoint http://127.0.0.1:4107 \
    --credential-file "$app_etc/registration.credential"
fi

echo "== app"
install -m 0644 "$deploy/greenhouse-body-double.service" /etc/systemd/system/greenhouse-body-double.service
systemctl daemon-reload
systemctl enable greenhouse-body-double
systemctl restart greenhouse-body-double
echo "installed; the owner approves the contract (deploy/body-double.md)"
