# Private browser package

Build on the Linux host with `npm run build` (requires wasm-pack and the
wasm32-unknown-unknown Rust target). Serve this directory through the
application's normal same-origin HTTP server; import `connect` from `client.js`.

```js
import { connect } from './client.js';
const session = await connect(document.querySelector('canvas'), authorizedWebSocketURL, {
  onready: name => console.log(name),
  onerror: error => console.error(error),
});
// On component destruction:
session.close();
```

The caller supplies the authorized websocket URL, handles clipboard permission
and reports connection errors. Text clipboard is Latin-1 as specified by RFB.
Passwords are encoded as UTF-8 bytes and truncated to eight bytes by VNC Auth.
DOM dead keys and composition/IME are not implemented. The package is private;
no npm publication or stormconsole replacement is performed by this build.
