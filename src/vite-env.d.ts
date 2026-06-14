/// <reference types="vite/client" />

// zmodem.js ships pure CommonJS with no bundled types. Treat it as any
// — the API surface we use (Session.parse, set_sender, consume, send_offer,
// accept, send, end, abort, on, type) is exercised through ZmodemBridge.
declare module "zmodem.js" {
  const Zmodem: any;
  export default Zmodem;
}

