/**
 * The app's button icon vocabulary: one intent, one icon, everywhere.
 *
 * Icons are named for what the button DOES, not for what they look like -
 * `IconSave`, not `IconCheck`. Two buttons that do the same thing then read
 * as the same import, and a screen that invents a second icon for an action
 * that already has one is obvious in review.
 *
 * They sit ALONGSIDE the label, never instead of it. An icon makes a button
 * quicker to FIND; only a small universal set (close, search, print) is
 * quicker to understand, and almost nothing here is in it. "Share for
 * review" has no glyph anyone would guess.
 *
 * Every use is `aria-hidden`: the label already names the action, so an
 * announced icon would only repeat it - and the accessible name stays
 * exactly what it was before icons existed.
 *
 * Size comes from the Button (`[&_svg]:size-*`), so call sites pass no
 * className.
 */
export {
  // Getting data in and out
  Import as IconImport,
  Download as IconExport,
  // Fetching something to this machine (the How To Use guide) - the same
  // arrow as exporting, named for what the button does.
  Download as IconDownload,
  ExternalLink as IconOpenInBrowser,
  Share2 as IconShare,
  Copy as IconCopy,
  FolderOpen as IconBrowse,
  FileText as IconReport,
  // Saving a report of a run as a file (Auto Run's Past runs) - a page with
  // a down arrow, since it writes a file rather than opening one to read.
  FileDown as IconExportReport,
  AppWindow as IconOpenWindow,

  // Making and changing things
  Plus as IconAdd,
  Check as IconConfirm,
  Pencil as IconEdit,
  Layers as IconBulkEdit,
  CaseSensitive as IconRename,
  Send as IconPost,
  // Setting what EVERY new item starts with, not editing this one. A pin
  // because the thing it opens fixes a value in place across cases.
  Pin as IconSetDefault,
  // Keeping a choice at the top of its list (a pinned board area). The
  // same picture as setting a default: a pin means this one stays put
  // where you can find it.
  Pin as IconPinToTop,
  // One step up or down in an ordered list: the keyboard's route to the
  // same move a drag makes.
  ArrowUp as IconMoveUp,
  ArrowDown as IconMoveDown,
  // Choosing the order a list of cases runs in, in one dialog: a numbered
  // list, because what it sets is which case comes first.
  ListOrdered as IconSetOrder,
  FolderPlus as IconNewSuite,
  // Copying selected cases into another suite: a folder with an arrow in.
  FolderInput as IconCopyToSuite,

  // Undoing and stopping
  Undo2 as IconUndo,
  X as IconCancel,
  ChevronsDownUp as IconCollapseAll,
  // The opposite of IconCollapseAll: every folded group or lane opens.
  ChevronsUpDown as IconExpandAll,
  ArrowRightLeft as IconMoveToPbi,
  Eraser as IconClear,
  Trash2 as IconRemove,
  // Taking a note out of use while keeping it to bring back (Auto Run's
  // Known quirks) - an archive box, since nothing is deleted.
  Archive as IconRetire,
  ArchiveRestore as IconRestore,
  EyeOff as IconStopWatching,
  Square as IconStop,
  Unplug as IconUnregister,
  // Wiping saved script FILES for a PBI's cases (Auto Run, dev builds only).
  FileX2 as IconClearScripts,
  // Wiping the saved run RECORDS and their screenshots (Auto Run, dev
  // builds only). `Eraser` is already IconClear for something else, so
  // this gets its own glyph rather than reusing that one.
  ListX as IconClearResults,
  // Lifting a hold the user has manually verified is safe to lift - an
  // unlocked padlock, not a check mark, because nothing was confirmed BY
  // this app.
  Unlock as IconRelease,

  // Moving through a flow
  ChevronLeft as IconBack,
  ChevronRight as IconNext,
  ClipboardCheck as IconReview,
  Play as IconRun,
  // Starting a run nobody has to sit in front of - a person picks IconRun.
  Bot as IconUnattended,
  Flag as IconFinish,

  // Capturing evidence during a run
  Video as IconRecord,
  Scissors as IconSnip,
  ClipboardPaste as IconPasteImage,
  Paperclip as IconAttach,
  // Adding a wiki page as a spec reference - a plain link glyph, since the
  // button pastes a URL rather than picking a file.
  Link as IconWikiLink,

  // Sending a reviewed local result out to Azure DevOps. `Send` is already
  // IconPost (posting a comment) - this is a different action, so it gets
  // its own glyph rather than reusing that one for something else.
  SendHorizontal as IconSendResults,

  // Everything else
  // Opening the calendar to choose a date (DateField).
  CalendarDays as IconPickDate,
  LogIn as IconSignIn,
  RefreshCw as IconRefresh,
  Plug as IconRegister,
  Compass as IconTour,
  // Opening the bundled "How To Use" help site in the browser.
  BookOpen as IconHelp,
  Bug as IconBug,
  KanbanSquare as IconBoard,
  // The tester's own test accounts, and the project's way of signing in.
  Users as IconAccounts,
  KeyRound as IconRecipe,
  // The menu paths an unattended run follows to each module's screen.
  Route as IconModulePaths,
  // A project's Test files: the documents scripts and API templates upload
  // into the application (Auto Run's Setup card, the API Templates tab).
  Files as IconTestFiles,
  // Copying files from this machine into those Test files.
  FileUp as IconAddFiles,
  // The site a project's runs sign in to and start from (Auto Run's
  // Setup card) - a globe, since what it opens is a web address.
  Globe as IconSiteAddress,
  // Choosing something on a page by clicking it in the browser - a
  // recorded sign-in's signed-in check.
  MousePointerClick as IconPickOnPage,
  // Opening the bundled game in Settings' optional extras.
  Gamepad2 as IconPlayGame,
  // The app's own mark, for the way BACK to the test case side - the
  // mode switch names its destination, so its icon has to as well.
  FlaskConical as IconTestCases,
} from "lucide-react";
