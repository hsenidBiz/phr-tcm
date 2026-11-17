/**
 * Plain definitions of the words the Auto Run and API Templates screens use
 * without explaining. Written for a tester, in the wording the How To Use
 * guide uses (`docs-site/src/content`), and shown by `Term`
 * (`src/components/Term.tsx`) where each word first appears on a screen.
 *
 * One or two sentences each. Change a definition here and the guide's own
 * sentence for the same word should follow it.
 */
export const GLOSSARY = {
  recipe:
    "The saved way to sign in to this application: what to fill in, what to click, and what only a signed-in page shows. It is recorded once by signing in by hand, and every script uses it.",
  template:
    "A saved way of making or changing test data through the application's own requests, such as creating a user or locking an account. An assistant builds each one and proves it on your site first.",
  fixture:
    "A draft an assistant has taught the app to build for a script: the templates to run, in order, and what they make. The script's case reads that draft's values when it runs.",
  flow: "The path a record takes through a part of the application, such as a wizard, as stages in the order the application allows. Each stage has a check that tells whether it is done for a record.",
  area: "A named place inside a module, such as one screen, where a case can start. The app records how to reach it by following the site's menu once.",
  component:
    "A reusable step, such as picking a date in a date picker or confirming a dialog. Scripts use it by name, and every script that uses it gets its fixes.",
  quirk:
    "A note about how this application behaves, kept so the next script or template does not have to find it out again.",
  divergence:
    "A place where a test case and the live application disagree, such as a different value at a step. An assistant reports it instead of bending the check to fit the page.",
  ruling:
    "Your decision about a divergence: the app is wrong, the app is right and the case needs updating, or you fixed the data.",
} as const;

export type TermName = keyof typeof GLOSSARY;
