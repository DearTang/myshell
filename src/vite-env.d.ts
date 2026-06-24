/// <reference types="vite/client" />

// Vite `?raw` suffix imports a file's contents as a string at build time.
// Used to bundle CHANGELOG.md into the frontend for the about/whats-new dialog.
declare module "*.md?raw" {
  const content: string;
  export default content;
}

// zmodem.js ships pure CommonJS with no bundled types. Treat it as any
// — the API surface we use (Session.parse, set_sender, consume, send_offer,
// accept, send, end, abort, on, type) is exercised through ZmodemBridge.
declare module "zmodem.js" {
  const Zmodem: any;
  export default Zmodem;
}

