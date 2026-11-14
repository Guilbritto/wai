# wai-relay

Small relay behind the WayCarbon "Pedir ajuda" feature: colleagues send plain-text help requests to each other. The extension signs in with GitHub (device flow), then keeps one `GET /events` stream open for live updates. Only members of the GitHub org can use it.

## Build and run

    go build ./relay        # from extensions/wai-waycarbon
    RELAY_DATA=/var/lib/relay/relay.json ... ./relay

TLS is ended by the load balancer; the relay speaks plain HTTP on `RELAY_ADDR`. Stop it with SIGTERM or SIGINT (graceful shutdown).

## Environment

| Variable | Required | Meaning |
|---|---|---|
| `RELAY_ADDR` | no | Listen address, default `:8080` |
| `RELAY_DATA` | yes | Path of the JSON store (users, token hashes, requests); needs a durable volume |
| `RELAY_ORG` | yes | GitHub org whose members may sign in |
| `RELAY_GITHUB_CLIENT_ID` | yes | Client ID of the GitHub App (device flow) |
| `RELAY_GITHUB_APP_ID` | yes | GitHub App ID |
| `RELAY_GITHUB_INSTALLATION_ID` | yes | Installation ID of the app on the org |
| `RELAY_GITHUB_APP_KEY` | yes | Path to the app's PEM private key (PKCS#1 or PKCS#8) |
| `RELAY_GITHUB_API` | no | Default `https://api.github.com` |
| `RELAY_GITHUB_WEB` | no | Default `https://github.com` |

## GitHub App

- Device flow enabled.
- Organization permission: Members, read.
- Installed on the org.
- No repository permissions.

## Behavior

- Membership is checked at sign-in, on every `/events` connect, on every write under `/requests`, and hourly for connected users. A user who left the org gets 403, loses their streams and has their tokens dropped.
- Requests expire after 24 h if not accepted and are deleted 7 days after they end. At most 4 streams per user, 20 requests per hour per sender, 10 waiting per recipient, 8000 characters of text, 64 KiB bodies.
- Logs carry method, path, status and errors only: never request text, replies, reasons, tokens or device codes.

## Owner-only items

- Create the GitHub App, install it on the org and generate its private key.
- Deploy the relay with the variables above and a persistent `RELAY_DATA` volume.
- Put it behind a load balancer that ends TLS; do not use a response timeout that cuts long-lived streams.
- Give the relay URL to the extension.
