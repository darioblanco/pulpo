// Example Pulpo webhook consumer → Discord.
//
// Reference for building your own integration on Pulpo's universal webhook: verify the
// signature, de-duplicate retries, filter by severity, and forward to any destination
// (here, a Discord webhook). Zero dependencies — Node 20+ built-ins only.
//
// Run:  node --env-file=.env index.mjs

import { createServer } from 'node:http';
import { createHmac, timingSafeEqual } from 'node:crypto';

const PORT = Number(process.env.PORT ?? 8099);
const SECRET = process.env.PULPO_WEBHOOK_SECRET;
const DISCORD_WEBHOOK_URL = process.env.DISCORD_WEBHOOK_URL;
const MIN_SEVERITY = process.env.MIN_SEVERITY ?? 'info';

if (!SECRET || !DISCORD_WEBHOOK_URL) {
  console.error('Set PULPO_WEBHOOK_SECRET and DISCORD_WEBHOOK_URL (see README).');
  process.exit(1);
}

const SEVERITY_RANK = { info: 0, warn: 1, critical: 2 };
const SEVERITY_EMOJI = { info: 'ℹ️', warn: '⚠️', critical: '🔴' };

// Idempotency: retries reuse X-Pulpo-Event-Id. In production use a durable store with TTL.
const seen = new Set();

/** Verify `X-Pulpo-Signature: sha256=<hex>` against the raw body (constant-time). */
function verifySignature(rawBody, header) {
  if (!header) return false;
  const expected = `sha256=${createHmac('sha256', SECRET).update(rawBody).digest('hex')}`;
  const a = Buffer.from(header);
  const b = Buffer.from(expected);
  return a.length === b.length && timingSafeEqual(a, b);
}

/** Turn the canonical envelope into a one-line Discord message. */
function formatMessage(event) {
  const emoji = SEVERITY_EMOJI[event.severity] ?? '•';
  const where = event.session?.name ? ` \`${event.session.name}\`` : '';
  const node = event.node ? ` on ${event.node}` : '';
  let detail = '';
  if (event.type === 'usage_alert') {
    const p = event.payload ?? {};
    if (p.cost_usd != null && p.budget_usd != null) {
      detail = ` — $${p.cost_usd} / $${p.budget_usd} budget`;
    } else if (p.quota_used_percent != null) {
      detail = ` — quota ${p.quota_used_percent}%`;
    }
  } else if (event.type === 'intervention') {
    detail = event.payload?.intervention_reason ? ` — ${event.payload.intervention_reason}` : '';
  }
  return `${emoji} **${event.type}.${event.subtype}**${where}${node}${detail}`;
}

async function postToDiscord(content) {
  const res = await fetch(DISCORD_WEBHOOK_URL, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ content }),
  });
  if (!res.ok) console.error(`Discord POST failed: ${res.status}`);
}

function readBody(req) {
  return new Promise((resolve, reject) => {
    const chunks = [];
    req.on('data', (c) => chunks.push(c));
    req.on('end', () => resolve(Buffer.concat(chunks).toString('utf8')));
    req.on('error', reject);
  });
}

const server = createServer(async (req, res) => {
  if (req.method !== 'POST') {
    res.writeHead(405).end();
    return;
  }
  const raw = await readBody(req);

  // 1) Verify signature — reject anything we can't authenticate.
  if (!verifySignature(raw, req.headers['x-pulpo-signature'])) {
    res.writeHead(401).end('bad signature');
    return;
  }

  // 2) De-dupe retries on the event id.
  const eventId = req.headers['x-pulpo-event-id'];
  if (eventId && seen.has(eventId)) {
    res.writeHead(200).end('duplicate');
    return;
  }
  if (eventId) seen.add(eventId);

  let event;
  try {
    event = JSON.parse(raw);
  } catch {
    res.writeHead(400).end('invalid json');
    return;
  }

  // 3) Severity filter (Pulpo also filters server-side; this is belt-and-suspenders).
  const rank = SEVERITY_RANK[event.severity] ?? 0;
  if (rank < (SEVERITY_RANK[MIN_SEVERITY] ?? 0)) {
    res.writeHead(200).end('below threshold');
    return;
  }

  // 4) Acknowledge fast, forward async (don't hold Pulpo's delivery open on Discord).
  res.writeHead(200).end('ok');
  postToDiscord(formatMessage(event)).catch((e) => console.error(e));
});

server.listen(PORT, () => {
  console.log(`Listening for Pulpo webhooks on :${PORT}/  (min severity: ${MIN_SEVERITY})`);
});
