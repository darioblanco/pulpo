# API Reference

Pulpo exposes REST + SSE from `pulpod` (default `:7433`).

Common endpoints:

- `GET /api/v1/health`
- `GET /api/v1/sessions`
- `POST /api/v1/sessions`
- `GET /api/v1/sessions/:id`
- `POST /api/v1/sessions/:id/kill`
- `POST /api/v1/sessions/:id/resume`
- `GET /api/v1/sessions/:id/interventions`
- `GET /api/v1/events` (SSE)
- `GET /api/v1/inks`

For payload-level behavior and lifecycle semantics, see:

- [SPEC.md](https://github.com/darioblanco/pulpo/blob/main/SPEC.md)
