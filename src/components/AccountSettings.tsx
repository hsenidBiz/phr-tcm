import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { commands, type AppSettings, type AuthStatus } from "../bindings";
import { authQuery, KEPT_ON_SIGN_OUT } from "../lib/authQuery";
import { toast } from "../lib/toast";
import { Button } from "./ui/button";
import { Switch } from "./ui/switch";
import { SettingRow } from "./settings/SettingsCard";

const SIGNED_OUT: AuthStatus = { signed_in: false, account: null };

/** The sign-in rows of Settings' General card: who is signed in, with Sign
 *  out, and Stay signed in - whether the sign-in is kept in Windows
 *  Credential Manager so a launch goes straight in. Both are kept by Rust
 *  (commands/auth.rs, saved_session.rs); the token itself never reaches
 *  this side. Rows, not a section, like BackgroundSettings. */
export default function AccountSettings() {
  const qc = useQueryClient();
  const auth = useQuery(authQuery);
  const settings = useQuery({ queryKey: ["app-settings"], queryFn: () => commands.getAppSettings() });

  const setStay = async (on: boolean) => {
    const before = settings.data;
    if (before) qc.setQueryData<AppSettings>(["app-settings"], { ...before, stay_signed_in: on });
    try {
      const r = await commands.setStaySignedIn(on);
      if (r.status === "error") throw r.error;
      qc.setQueryData(["app-settings"], r.data);
    } catch (e) {
      if (before) qc.setQueryData(["app-settings"], before);
      toast.error(String(e));
    }
  };

  const signOut = useMutation({
    mutationFn: async () => {
      const r = await commands.signOut();
      if (r.status === "error") throw new Error(r.error);
      return r.data;
    },
    // Either way the session is gone from this run (Rust clears it before
    // anything can fail), so the sign-in screen is right either way. The
    // previous person's data goes from memory with them: the next sign-in
    // may be someone else, and a screen seeded from the old answers would
    // show it to them before its first refetch.
    onSettled: (data) => {
      qc.removeQueries({ predicate: (q) => !KEPT_ON_SIGN_OUT.has(String(q.queryKey[0])) });
      qc.setQueryData(["auth"], data ?? SIGNED_OUT);
    },
    onError: (e) => toast.error(e.message),
  });

  const account = auth.data?.account;
  return (
    <>
      <SettingRow
        name={account ? `Signed in as ${account}` : "Signed in"}
        description="Sign out to switch accounts. The next sign-in asks which account to use."
        control={
          <Button size="sm" variant="outline" disabled={signOut.isPending} onClick={() => signOut.mutate()}>
            {signOut.isPending ? "Signing out" : "Sign out"}
          </Button>
        }
      />
      <SettingRow
        asLabel
        name="Stay signed in"
        description="Opens the app without the browser while Microsoft still accepts your sign-in. Kept in Windows Credential Manager."
        control={
          <Switch
            checked={settings.data?.stay_signed_in ?? true}
            disabled={!settings.data}
            onCheckedChange={(on) => void setStay(on)}
            ariaLabel="Stay signed in"
          />
        }
      />
    </>
  );
}
