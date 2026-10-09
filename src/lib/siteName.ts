/**
 * The site name of an address: its host, with no scheme, path, query or
 * port. Text that is not an address comes back as it was given, so a
 * placeholder or a half-typed entry still shows something.
 */
export function siteName(address: string): string {
  try {
    return new URL(address).hostname || address;
  } catch {
    return address;
  }
}
