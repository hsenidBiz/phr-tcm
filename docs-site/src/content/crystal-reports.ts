// Crystal Reports: the module that lets an AI assistant open, edit and
// preview Crystal Reports .rpt files, and the screen that shows the report
// it is working on. The assistant's side is on the AI Bridge; this page is
// the module, what the assistant can do, and the preview screen.
//
// Words come from screens/CrystalReports/index.tsx, the module card in
// screens/Modules and the assistant's tool list (src-tauri/src/mcp.rs).
// The screen has no shots yet: it needs an open report, which the capture's
// sample data does not have, so it is described in tips and how-tos only.
// Shown only while the Crystal Reports module is on (../modules.ts).

import type { Screen } from "../types";

export const crystalReports: Screen = {
  id: "crystal-reports",
  title: "Crystal Reports",
  group: "AI tools",
  summary:
    "Ask your AI assistant to open, change and preview a Crystal Reports file, and watch the report here as it changes. " +
    "The assistant does the work through the AI Bridge; this screen shows the pages it draws and opens the file in Crystal Reports when you want to look at it there. " +
    "Crystal Reports is a module: switch it on in Settings, under General, on the **Modules** page (it needs nothing else), and it appears at the end of the sidebar.",
  shots: [],
  controls: [],
  tips: [
    "The screen shows one report at a time, the one your assistant has open. With none open it says **No report open. Ask your AI assistant to open one.**",
    "The top of the screen shows the file name and, under it, the folder. Point at them to see the full path. Beside them it says where the pages came from: **Saved data** when they were drawn from the data stored in the report, or **Live data from** and the name of the database when they were drawn from fresh rows, and how many pages there are.",
    "The preview refreshes by itself whenever the assistant opens, changes or creates a report, or draws a new preview. You do not need to press anything.",
    "Notes from the drawing, such as an object it does not draw, are listed under the header. If the pages could not be drawn at all, the screen says **The preview could not be drawn.** and lists why instead of showing a page.",
    "**Open in Crystal Reports** opens the file in the program Windows has for .rpt files. If Windows has none, the screen says so; install Crystal Reports, or open the file from it. If the file has been moved or deleted since the assistant opened it, the screen says **The report file is no longer there.** instead of showing a preview.",
    "Every change the assistant makes is saved over the file at once, and TCM keeps no undo. The reports are meant to live in a git repository, and git is the undo: review the changes there before you commit them.",
    "Change a report from one place at a time. If you also edit the file in Crystal Reports, tell the assistant to open it again before it changes anything more.",
    "The preview is drawn by TCM, not by Crystal Reports, so it can differ a little from what Crystal Reports shows. What it cannot draw is listed in the notes.",
  ],
  howTo: [
    {
      title: "Switch Crystal Reports on",
      steps: [
        "Open **Settings**, then **Modules**, and switch **Crystal Reports** on. It needs no other module.",
        "**Crystal Reports** appears at the end of the sidebar, and your assistant gets its report tools. Switch the module off and both go away; nothing you have saved is deleted.",
        "Your assistant has to be connected to the AI Bridge as for any other tool: see **AI Bridge**.",
      ],
    },
    {
      title: "Have your assistant work on a report",
      steps: [
        "Give your assistant the full path to the .rpt file and say what to change. It opens the report, reads its areas, sections and objects, changes them and checks the result.",
        "It can open and read a report, change text, fonts, colours, sizes, positions, borders, sections, groups, sorting, formulas, record selection, tables and page setup, add new objects, compare two reports, run the offline load checks and create a blank report. It reads its own guide first, which also tells it what it cannot do.",
        "Open **Crystal Reports** in the sidebar to see the pages. Each time the assistant changes the report, or asks for a new preview, the pages here are redrawn.",
        "Ask it to check the report before you call the work done. The check lists errors, where Crystal Reports would refuse or garble the file, and warnings, which are suspect but usually fine.",
      ],
    },
    {
      title: "Preview a report with live data",
      steps: [
        "A preview is normally drawn from the data saved in the report. To see fresh rows, ask your assistant for a live preview: it runs the report's stored procedure on the database chosen on the AI Bridge tab and draws what comes back.",
        "A live preview runs a procedure, which counts as a database change. It works only when **Database Access** is on, **Create, update and delete** is on under Database Read Access on the AI Bridge tab, and the chosen connection is the Dev login. Anything else is refused, and the refusal says what to change.",
        "Only a stored procedure of the connection's own database runs. A report that names another database or a linked server is refused before anything connects.",
        "The report's parameters come from the assistant's request. The values saved in the report are not used, so it must pass every parameter it wants set. A parameter the report does not have, or a required one left out, is refused.",
        "The screen then says **Live data from** and the connection's name, never the server's address. A page count of 0 means the drawing failed; the notes say why.",
      ],
    },
    {
      title: "Let your assistant change the report's database objects",
      steps: [
        "A report is often only as good as the stored procedure, view or function behind it. On the Dev login, with **Database Access**, **Crystal Reports** and **Create, update and delete** all on, your assistant may create and alter procedures, views and functions through the database tool.",
        "On any other login it may not: those changes are refused, and the refusal says what to switch on.",
        "Every statement is written to the app's log, as for any other database change.",
      ],
    },
    {
      title: "Open the report in Crystal Reports",
      steps: [
        "On the **Crystal Reports** screen, press **Open in Crystal Reports**. The file opens in the program Windows has for .rpt files.",
        "Save and close it there before you ask the assistant for more changes, and tell the assistant to open the report again so it reads what you saved.",
      ],
    },
    {
      title: "What the assistant cannot do yet",
      steps: [
        "Text with fields embedded in it is read-only: the assistant can see it but cannot rewrite it, because that would drop the fields.",
        "The insides of a subreport cannot be changed from the report that holds it.",
        "Paper size and orientation cannot be changed on a report that was saved without a printer.",
        "Password-protected reports, and reports older than Crystal Reports XI, cannot be opened.",
        "A report can be edited only if TCM can write the file. A file that is locked or read-only is refused, and the file is left as it was.",
      ],
    },
  ],
};
