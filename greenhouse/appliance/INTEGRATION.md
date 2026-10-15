# Appliance side of Body double: how to wire it in

The Fedora bootc image builds the Body double app, installs its unit and enables it. Like every
Greenhouse service in the image, the unit is skipped, not failed, until the app's credential is on
the machine. Behaviour: [`apps/body-double.md`](../apps/body-double.md); VM140 runbook:
[`deploy/body-double.md`](../deploy/body-double.md).

| File here | What to do with it |
|---|---|
| `rootfs/usr/lib/systemd/system/greenhouse-body-double.service` | New. Copy to the same path. The same bytes as `deploy/greenhouse-body-double.service`. Nothing generates the appliance units, so keep the two in step by hand. |
| `rootfs/usr/lib/systemd/system/greenhouse-body-double.service.d/05-appliance-restored.conf` | New. Copy to the same path. The same text `appliance_units.py` wrote for the other nine units, with this app's credential. |
| `INTEGRATION.md` | This file. Do not copy. |

The edits below change four existing files. Each "Before" is quoted from `appliance/` as last
seen in the owner's sessions (main checkout, 2026-10-15; timestamps with each step). Those
files may have moved since then. Check each "Before" still matches once, then replace it with
its "After". The anchors were applied to the reconstructed files and checked (see "Checked so
far" at the end).

Nothing else changes:
- no per-machine firewall set: the app reaches only the kernel on loopback, so `firewall-site` and
  `/etc/greenhouse/machine/firewall.conf` stay as they are;
- `containerignore` already keeps `target/`, `*.db` and `*.sqlite3` out of the build context;
- `body-double-app` has no path dependency but `kernel`, which the build stage already copies, and
  it includes no file from outside its crate. Its crates are all in `Cargo.lock` already
  (`rusqlite` 0.37 with `bundled`, as node-monitor uses it).

## 1. `Containerfile`, the `greenhouse-rust` stage

Reconstructed final: the recon Write 2026-10-15T01:23:32Z plus its Edits through 02:04:34Z
(reports2/deploy-runtime.md §9.2). Three edits.

The crate's sources.

**Before**

```dockerfile
COPY web-search-app web-search-app
COPY weather-app weather-app
COPY deploy deploy
```

**After**

```dockerfile
COPY web-search-app web-search-app
COPY weather-app weather-app
COPY body-double-app body-double-app
COPY deploy deploy
```

The package in the build, and its binary in the list copied to `/opt/greenhouse/bin/`.

**Before**

```dockerfile
      -p home-assistant-app -p weather-app -p web-search-app; \
    mkdir /out; \
    for bin in greenhouse-kernel greenhouse-admin inference-gateway lifecycle-helper node-monitor node-agent \
               greenhousectl greenhouse-home-assistant greenhouse-weather greenhouse-web-search; do \
```

**After**

```dockerfile
      -p home-assistant-app -p weather-app -p web-search-app -p body-double-app; \
    mkdir /out; \
    for bin in greenhouse-kernel greenhouse-admin inference-gateway lifecycle-helper node-monitor node-agent \
               greenhousectl greenhouse-home-assistant greenhouse-weather greenhouse-web-search \
               greenhouse-body-double; do \
```

## 2. `Containerfile`, the final stage

The unit is enabled with the others (same source as step 1).

**Before**

```dockerfile
        greenhouse-node-monitor.service greenhouse-home-assistant.service greenhouse-weather.service \
        greenhouse-web-search.service greenhouse-accounts-helper.path; \
```

**After**

```dockerfile
        greenhouse-node-monitor.service greenhouse-home-assistant.service greenhouse-weather.service \
        greenhouse-web-search.service greenhouse-body-double.service greenhouse-accounts-helper.path; \
```

## 3. `rootfs/usr/lib/sysusers.d/greenhouse-apps.conf`

The app's user. VM140 creates it the way it created weather's, by running `systemd-sysusers` on
`deploy/greenhouse.sysusers`, not from `/etc/sysusers.d`, so it belongs in this file. Written by
`appliance_units.py` (recon Write 2026-10-15T01:21:44Z; reports2/deploy-runtime.md §9.6).

**Before**

```text
u greenhouse-web-search - "Greenhouse web search app" /var/lib/greenhouse/apps/web-search
```

**After**

```text
u greenhouse-web-search - "Greenhouse web search app" /var/lib/greenhouse/apps/web-search
u greenhouse-body-double - "Greenhouse body double app" /var/lib/greenhouse/apps/body-double
```

## 4. `rootfs/usr/lib/tmpfiles.d/greenhouse-apps.conf`

The app's configuration folder, which the VM140 installer makes by hand. The state folder needs no
line: the unit's `StateDirectory=` makes it, owned by the app's user, 0700. Same source as step 3.

**Before**

```text
d /etc/greenhouse/apps/web-search 0750 root greenhouse-web-search -
d /etc/greenhouse/machine 0755 root root -
```

**After**

```text
d /etc/greenhouse/apps/web-search 0750 root greenhouse-web-search -
d /etc/greenhouse/apps/body-double 0750 root greenhouse-body-double -
d /etc/greenhouse/machine 0755 root root -
```

## 5. `rootfs/etc/nftables/greenhouse-egress.nft`

The set the unit's `NFTSet=` fills, and the app's rules: the kernel on loopback, then drop. The
same as `deploy/body-double/egress.nft`. The file was generated by `appliance_egress.py` (recon
Write 2026-10-15T01:22:50Z) from VM140's table (prod `cat` 2026-10-15T01:20:35Z), which the
script leaves unchanged at both anchors (reports2/deploy-runtime.md §6.2, §9.5).

The set, after the last set.

**Before**

```nft
    set cloud_gateway_cgroups {
        type cgroupsv2
        comment "systemd-managed greenhouse-cloud-gateway.service cgroups"
    }

    chain output {
```

**After**

```nft
    set cloud_gateway_cgroups {
        type cgroupsv2
        comment "systemd-managed greenhouse-cloud-gateway.service cgroups"
    }
    set body_double_cgroups {
        type cgroupsv2
        comment "systemd-managed greenhouse-body-double.service cgroups"
    }

    chain output {
```

The rules, at the end of chain `output`.

**Before**

```nft
        socket cgroupv2 level 2 @cloud_gateway_cgroups counter drop
    }
}
```

**After**

```nft
        socket cgroupv2 level 2 @cloud_gateway_cgroups counter drop
        socket cgroupv2 level 2 @body_double_cgroups ct state established,related accept
        socket cgroupv2 level 2 @body_double_cgroups ip daddr 127.0.0.1 tcp dport 4000 accept
        socket cgroupv2 level 2 @body_double_cgroups counter drop
    }
}
```

## 6. The lab check

`gh-layer-check.sh` is the session script that proved the Greenhouse layer in the lab VM (recon
Write 2026-10-15T01:36:39Z, run 01:49:21Z). It is not in the repository. Add the app to its two
lists, after `greenhouse-web-search`:

```bash
for u in greenhouse-kernel greenhouse-llmplayground greenhouse-node-monitor greenhouse-home-assistant greenhouse-weather greenhouse-web-search greenhouse-body-double caddy authelia greenhouse-searxng; do
for u in greenhouse-kernel greenhouse-llmplayground greenhouse-node-monitor greenhouse-home-assistant greenhouse-weather greenhouse-web-search greenhouse-body-double caddy authelia greenhouse-op-owner; do
```

and to `== firewall`:

```bash
nft list set inet greenhouse_egress body_double_cgroups
```

Expected on a fresh lab image:
- `== binaries`: `greenhouse-body-double 0 missing-libs`, eleven Rust binaries where there were
  ten;
- `== units`: `greenhouse-body-double       enabled no` (enabled, and skipped until restored);
- `== system`: `running`, with no failed units;
- `== firewall`: the set exists and is empty, since nothing in it runs;
- `== users`: `greenhouse-body-double` present, not `MISSING:`.

## What the image does not hold

Like the other apps, these are restored or provisioned in `/etc` and `/var`, or the unit stays
skipped:
- `/etc/greenhouse/apps/body-double/registration.credential` (the condition the unit waits on);
- `/etc/greenhouse/apps/body-double/config.json`, from `deploy/body-double/config.example.json`.
  The app refuses to start without it, and then restarts every 5 seconds, so restore it with the
  credential;
- the app slot in `/var/lib/greenhouse/kernel/kernel.db`, the approved contract and the owner's
  grants;
- the journal, `/var/lib/greenhouse/apps/body-double/body-double.sqlite3`. It is personal and
  belongs on the passkey-unlocked tier-2 volume once the appliance has one (proposal, decision 4).
  Until then it sits in `/var` with the other apps' state.

A fresh appliance has no way yet to provision an app slot without a restore. That gap is the
appliance's, not this app's.

## Checked so far

Here, against the reconstructed `appliance/` files (not a real build or boot):
- each "Before" above occurs exactly once in its reconstructed file, and replacing it gives the
  "After";
- the edited `RUN` lines of the `Containerfile` parse with `bash -n`;
- the edited `greenhouse-egress.nft` passes `nft -c -f`;
- `systemd-sysusers` and `systemd-tmpfiles` on a scratch root, with the edited
  `greenhouse-apps.conf` files, create the user and `/etc/greenhouse/apps/body-double` as
  `root:greenhouse-body-double` 0750;
- `systemd-analyze verify` on the unit with its drop-in reports only the missing binary.

Still to prove in the lab VM: the image builds with `-p body-double-app`, and the layer check
reads as above.
