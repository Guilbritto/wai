# wai-escritorio

A shared virtual office for wai. Everyone on the team who runs this extension walks around the same
pixel-art map, sees who is nearby, and shows a coarse status: Focused or Needs you. Nothing else
about your sessions leaves your machine: no agent names, session ids or titles.

## Install

Build the extension and put the binary where the manifest expects it:

```
cd extensions/wai-escritorio
go build -o bin/wai-escritorio .
```

Then install the folder as a wai extension. On first run the sidebar shows a setup form: name (1 to
32 characters), server address, team token and avatar. The setup is saved in the extension data
directory with mode 0600. After saving, the token is never shown again: "Change setup" prefills
name, server and avatar and leaves the token empty; submit with the token empty to keep the saved one.

Click "Enter office" to open the map pane, then walk with the arrow keys or WASD.

## Run the server

```
go build -o wai-escritorio-server ./server
./wai-escritorio-server -config office.json
```

`office.json`:

```json
{
  "listen": "0.0.0.0:8443",
  "token": "at-least-32-characters-of-shared-secret",
  "tls_cert": "/etc/office/cert.pem",
  "tls_key": "/etc/office/key.pem",
  "map": ""
}
```

- `token`: shared team secret, at least 32 characters. The server refuses to start with a shorter one.
- `tls_cert` and `tls_key`: set both to serve `wss://`. Clients refuse plain `ws://` to any host
  that is not on the local computer, so use TLS for anything remote.
- `map`: path to a custom map file; empty uses the built-in office.
- `listen`: use `127.0.0.1:0` to pick a free port; the bound address is printed on stderr as
  `listening on <addr>`.

Self-signed certificate: the client verifies the certificate normally, so a self-signed one is
rejected ("Cannot reach <address>"). Either use a certificate from a CA the machine trusts, or add
your own CA to the system trust store on every client.
