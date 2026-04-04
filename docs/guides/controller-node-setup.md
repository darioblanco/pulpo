# Controller + Node Setup

Pulpo’s multi-node control plane is intentionally simple: every node runs the same binary, one node becomes the controller, and the controller keeps the canonical session index while managed nodes run the actual tmux/docker backends.

This guide walks through the end-to-end flow so you can get a controller and node running together without guessing which config goes where.

## 1. Pick a controller node

Choose the machine you want to own fleet visibility and cross-node writes. On that machine’s `~/.pulpo/config.toml`, enable controller mode:

```toml
[controller]
enabled = true            # promotes this node to controller mode
stale_timeout_secs = 300  # how long before a silent node is marked lost
```

Start `pulpod` and verify it identifies itself as a controller in `pulpo nodes enrolled` and the web UI. The controller keeps the SQLite session index on disk, so it can recover across restarts even while managed nodes continue running their sessions.

## 2. Enroll a managed node

Run the enrollment CLI from the controller. The command both creates the token and reports how to configure the target node:

```bash
pulpo --node controller-name nodes enroll gpu-box

# Output includes
# node token: abc123
# Use this token on the managed node in [controller].token
```

Repeat for each node you plan to manage. The controller stores the `node-token` and the last-seen address in its SQLite registry.

## 3. Configure the managed node

On the managed node, point it at the controller and paste the issued token:

```toml
[controller]
address = "https://controller-name.tailnet.ts.net"
token = "abc123"
```

`node` mode requires `controller.token` even in `tailscale` because these tokens are the application-layer identity. Restart `pulpod` after updating the config; the node will immediately push heartbeats and session events to the controller and poll for pending commands.

## 4. Verify the controller/node relationship

Use the controller’s CLI/web UI for fleet-wide work:

- `pulpo nodes enrolled` shows every enrolled node’s last seen address and status.
- `pulpo list --node gpu-box` routes through the controller to view or stop sessions running on `gpu-box`.
- The controller UI still has local session tables for itself, but the remote session detail links go through the controller’s HTTP proxy, keeping sensitive secrets on managed nodes.

On the managed node, local sessions stay visible and manageable. Remote read-only commands (list/logs/status) continue to work via `--node` and the existing `pulpo` CLI options, but anything that touches another node uses the controller.

## 5. Troubleshooting

- **Token mismatch**: the controller log shows `invalid node token`; re-run `pulpo nodes enroll` and copy the new token exactly.
- **Missing `controller.token`**: `pulpod` refuses to start; the log mentions the configuration requirement. Add the token and restart.
- **Managed node not showing as `online`**: verify the node can reach the controller address (firewall, Tailscale ACLs) and that its certificate matches the controller’s token.
- **Remote actions failing**: double-check that cross-node commands (create/stop/resume) are being sent to the controller, not the managed node directly. The controller-side logs and the `controller` UI will show the `node_id` that handled the request.

## Next steps

Once the controller/node pair is healthy, you can point schedules, web UI, and API clients at the controller for fleet-wide visibility, while still keeping each node usable for local sessions. For more configuration detail, see the [Configuration Guide](/guides/configuration) and the [Discovery Guide](/guides/discovery).
