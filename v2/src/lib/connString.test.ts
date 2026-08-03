import { expect, test } from "vitest";
import { buildConnString, isRepresentable, parseConnString } from "./connString";

test("fields build the standard SQL Server string", () => {
  expect(
    buildConnString({
      host: "phrx-db.internal",
      port: "1433",
      database: "PHRX",
      user: "tcm_reader",
      password: "s3cret",
      trustCert: true,
      extras: "",
    }),
  ).toBe(
    "Server=phrx-db.internal,1433;Database=PHRX;User Id=tcm_reader;Password=s3cret;TrustServerCertificate=True;",
  );
});

test("a stored string parses back into the same fields", () => {
  const f = parseConnString(
    "Server=phrx-db.internal,1433;Database=PHRX;User Id=tcm_reader;Password=s3cret;TrustServerCertificate=True;",
  );
  expect(f.host).toBe("phrx-db.internal");
  expect(f.port).toBe("1433");
  expect(f.database).toBe("PHRX");
  expect(f.user).toBe("tcm_reader");
  expect(f.password).toBe("s3cret");
  expect(f.trustCert).toBe(true);
  expect(f.extras).toBe("");
});

/// The builder must never destroy an option it does not understand - the
/// one database it was built for may depend on it.
test("unknown options survive the round trip verbatim", () => {
  const raw =
    "Server=h;Database=d;User Id=u;Password=p;Encrypt=False;Connection Timeout=60;";
  const f = parseConnString(raw);
  expect(f.extras).toBe("Encrypt=False; Connection Timeout=60");
  const rebuilt = buildConnString(f);
  expect(rebuilt).toContain("Encrypt=False;");
  expect(rebuilt).toContain("Connection Timeout=60;");
  expect(isRepresentable(raw)).toBe(true);
});

test("aliases and a bare host are read; a port is only split when numeric", () => {
  const f = parseConnString("Data Source=host\\instance;Initial Catalog=db;UID=u;PWD=p");
  expect(f.host).toBe("host\\instance");
  expect(f.port).toBe("");
  expect(f.database).toBe("db");
  expect(f.user).toBe("u");
  expect(f.password).toBe("p");
  expect(f.trustCert).toBe(false);
});

test("an empty builder produces an empty string, not a lone semicolon", () => {
  expect(
    buildConnString({ host: "", port: "", database: "", user: "", password: "", trustCert: true, extras: "" }),
  ).toBe("");
  expect(isRepresentable("")).toBe(true);
});

/// A password can legitimately contain '=' - only the FIRST '=' in a pair
/// splits key from value.
test("a password containing = survives", () => {
  const f = parseConnString("Server=h;Password=a=b=c;");
  expect(f.password).toBe("a=b=c");
  expect(buildConnString(f)).toContain("Password=a=b=c;");
});
