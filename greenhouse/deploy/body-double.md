# Body double app on VM140

Behaviour: [`apps/body-double.md`](../apps/body-double.md). The owner's decisions:
[`docs/design/body-double-proposal.md`](../docs/design/body-double-proposal.md).

**Status:** not deployed. Nothing is registered, approved or granted.

| Piece | What | Reach |
|---|---|---|
| `greenhouse-body-double.service` | the app (`body-double-app/`), user `greenhouse-body-double`, port 4107 on loopback | The kernel on `127.0.0.1:4000` only. No name lookups, no public web, never the LAN or the tailnet |
| `/etc/greenhouse/apps/body-double/` | `config.json` (0640 root:greenhouse-body-double) and the registration credential | n/a |
| `/var/lib/greenhouse/apps/body-double/` | the journal, `body-double.sqlite3` (WAL), 0700 | n/a |

The app holds no secrets and runs no model. Its only calls are to the kernel: registration,
`/api/notify` and `/api/notify/retire`. The kernel calls it for tools and for the check-in
schedule, and its proxy relays a person's journal download (`/apps/body-double/export`).

## Before the first install

`deploy/greenhouse.sysusers` needs the app's user, after the weather app's line:

```text
u greenhouse-body-double - "Greenhouse body double app" /var/lib/greenhouse/apps/body-double
```

The line travels with this delivery as `deploy/greenhouse.sysusers.d/body-double.conf`, because
the shared file cannot be copied whole. Merge the line; do not copy that file into the repo. The
installer stops with a message if the user is still missing after `systemd-sysusers`.

## Install

On VM140, from a release clone (`umask 022`), in the same environment as the other app builds:

```bash
cargo build --release --locked --offline -p body-double-app --bin greenhouse-body-double
deploy/body-double/install-body-double.sh /var/lib/greenhouse-app-build/target/release/greenhouse-body-double
```

The installer:
- creates the user `greenhouse-body-double`, installs the binary into `/opt/greenhouse/bin/` and
  restores its SELinux label, and creates `/etc/greenhouse/apps/body-double/config.json` from
  `config.example.json` if there is none (zone `America/New_York`, notices for `owner`, check-ins
  every 45 minutes, quiet 22:00–08:00). `notices_for` names one person, the owner's user id; the
  app refuses a list;
- adds the `body_double_cgroups` egress set and rules to `greenhouse-production-egress.nft` (a
  backup is kept) and to the running ruleset with `nft add`. Those rules allow the kernel only;
- provisions the app's kernel slot (`--endpoint http://127.0.0.1:4107`) and starts the app.

Run it again to update the binary: it keeps the config, the egress rules and the credential, and
restarts the app. Keep the old binary first, as the other deploys do
(`cp -p /opt/greenhouse/bin/greenhouse-body-double /opt/greenhouse/bin/greenhouse-body-double.pre-<sha>`).

On start the app logs `internal app API on http://127.0.0.1:4107`, then `registered with kernel`.
Registering creates a **pending contract**:
- nine tools: `now`, `day` (read); `log`, `set_task`, `update_task`, `open_thread`,
  `close_thread`, `checkins` (write); `checkin_tick` (command, scheduled only);
- the schedule `checkins.tick`, cron `*/5 * * * *`, action `checkin_tick`;
- notify `in-app`, category `body-double-checkin`, sensitivity `private`;
- no secrets, no egress, no `model_compute`.

Nothing works until the owner approves it.

## Owner steps

As `greenhouse-op-owner`, like the other apps. Each command reads the previous state first:

```bash
greenhousectl apps proposals body-double         # the pending proposal: its seq, hash and manifest
greenhousectl apps approve body-double --operation-id ... --proposal-seq <seq> --proposal-hash <hash> \
    --expected-contract-rev 0 --expected-contract-hash none
greenhousectl grants list --user owner --app body-double
greenhousectl grants set owner body-double --effect read --effect write --operation-id ... --expected-hash <state_hash>
```

`--effect` is repeated once per effect, and `grants set` sets the effects named. Grants are not
cumulative: `read` offers `now` and `day` and allows the journal download; `write` offers the six
write tools. Nobody needs `command`: the scheduler calls `checkin_tick` as `system:scheduler`,
and the app refuses any other caller. The kernel would offer `checkin_tick` in chat to anyone
granted `command`, so grant it to nobody. Grant other people `read` and `write` only when they
ask.

For check-in notices to show in SayLLM, `body-double-checkin` must be on its shown categories
(`llmplayground/INTEGRATION.md`).

## Verify

Production checks are read-only. Never call a write tool or `checkin_tick` on production to test:
that would put entries in someone's journal or raise a real check-in.

1. The app is up and answers on loopback:

   ```bash
   systemctl is-active greenhouse-body-double
   journalctl -u greenhouse-body-double -o cat --no-pager | tail -5
   curl -s -m 3 http://127.0.0.1:4107/health        # {}
   ```

2. It is enrolled. An unenrolled cgroup is unconfined (the chain policy is accept):

   ```bash
   nft list set inet greenhouse_egress body_double_cgroups   # elements = { "system.slice/greenhouse-body-double.service" }
   ```

3. Its reach, proven from inside the set with a throwaway unit, as for weather. The kernel answers
   (any HTTP status, 401 without a credential); everything else is blocked:

   ```bash
   systemd-run --quiet --collect --unit=gh-bd-nfttest -p Type=oneshot \
       -p NFTSet=cgroup:inet:greenhouse_egress:body_double_cgroups sh -c 'for u in http://127.0.0.1:4000/api/status http://127.0.0.1:4103/health http://192.168.1.125:4600/ http://192.168.1.118/ https://1.1.1.1/ https://api.weather.gov/; do curl -s -m 4 -o /dev/null -w "body-double cgroup -> $u %{http_code}\n" $u || echo "body-double cgroup -> $u blocked"; done'
   while systemctl is-active --quiet gh-bd-nfttest; do sleep 1; done
   journalctl -u gh-bd-nfttest --since "-2min" --no-pager -o cat | grep "body-double cgroup"
   nft list chain inet greenhouse_egress output | grep "body_double_cgroups counter"
   ```

   A blocked probe prints `000` and then `blocked`, and the set's `counter drop` rises. A name may
   still resolve through systemd-resolved's local socket; the connection is what is refused.

4. After the owner's steps, one read through the kernel, exactly as chat would dispatch it (grant
   check, identity, app). `now` changes nothing:

   ```bash
   CRED=$(cat /etc/greenhouse/kernel/proxy.credential)
   curl -s -m 30 -H "x-greenhouse-proxy-credential: $CRED" -H "x-greenhouse-authenticated-user: owner" \
       -H 'content-type: application/json' -d '{"args":{}}' \
       http://127.0.0.1:4000/api/tools/body-double.now/call \
     | python3 -c 'import json,sys; d=json.load(sys.stdin); r=d.get("result") or {}; print(d.get("ok"), d.get("error") or (r.get("now"), r.get("timezone"), r.get("checkins")))'
   ```

   `True`, the owner's local time and zone, and the check-in settings. Before the approval the
   kernel answers 404 `unknown tool 'body-double.now'`; before the grant, 403
   `denied — 'owner' lacks 'read' on 'body-double'`.

5. The schedule runs every five minutes as `system:scheduler`. Read-only, from the kernel's tool audit:

   ```bash
   python3 -c 'import sqlite3; k = sqlite3.connect("file:/var/lib/greenhouse/kernel/kernel.db?mode=ro", uri=True); print(k.execute("SELECT via, tool, ok, COUNT(*), MIN(ts), MAX(ts) FROM tool_audit WHERE app_id = ? AND via LIKE ? GROUP BY via, tool, ok", ("body-double", "schedule:%")).fetchall())'
   ```

   Rows with `schedule:checkins.tick`, `ok` 1, one every five minutes since the approval. While nobody has
   used the app nobody is due, so nothing is raised. An `ok` 0 row is a tick that could not send
   the owner's notice or record a check-in. The app's log says which
   (`journalctl -u greenhouse-body-double -o cat | grep "check-in tick"`), and the next tick tries
   again.

## Backups and the journal

The journal is personal and longitudinal. On VM140 it is in the app's state directory, covered by
the weekly VM backup with the other apps. Before an update that changes its schema, copy it with
SQLite's backup API, as for the kernel database. On the appliance it moves to the passkey-unlocked
tier-2 volume once that exists (proposal, decision 4).

## Rollback

1. `systemctl disable --now greenhouse-body-double`.
2. Restore `greenhouse-production-egress.nft.pre-body-double-<stamp>`. The running ruleset's extra
   set and rules are harmless once the service is gone. They disappear at the next nftables
   reload.
3. Revoke the app's credential:

   ```bash
   /opt/greenhouse/bin/greenhouse-admin revoke-app-credential --db /var/lib/greenhouse/kernel/kernel.db \
       --app-id body-double --credential-file /etc/greenhouse/apps/body-double/registration.credential
   mv /etc/greenhouse/apps/body-double/registration.credential \
      /etc/greenhouse/apps/body-double/registration.credential.revoked-<stamp>
   ```

   The revoked file is moved aside, so that a later run of `install-body-double.sh` provisions a
   new credential instead of starting the app with the dead one.

4. To withdraw the contract as well, the owner runs, as `greenhouse-op-owner`:

   ```bash
   greenhousectl apps revoke body-double --operation-id ... --expected-contract-rev <rev> \
       --expected-contract-hash <hash>
   ```

The journal stays in `/var/lib/greenhouse/apps/body-double/`. Delete it only on the owner's word.

## Appliance

The image builds the binary and enables the unit, which is skipped until the app's credential is
on the machine. The edits are in [`appliance/INTEGRATION.md`](../appliance/INTEGRATION.md).
