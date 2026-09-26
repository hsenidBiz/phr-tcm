// Provided by the `help-shots` plugin in docs-site/vite.config.ts: the ids
// of the shots that have an image in BOTH docs-site/shots/light/ and
// docs-site/shots/dark/. Any other shot renders as a placeholder frame.
declare module "virtual:help-shots" {
  const available: string[];
  export default available;
}
