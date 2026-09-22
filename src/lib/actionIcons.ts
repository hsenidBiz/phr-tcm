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
  ExternalLink as IconOpenInBrowser,
  Share2 as IconShare,
  Copy as IconCopy,
  FolderOpen as IconBrowse,
  FileText as IconReport,
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
  // One step up or down in an ordered list: the keyboard's route to the
  // same move a drag makes.
  ArrowUp as IconMoveUp,
  ArrowDown as IconMoveDown,
  FolderPlus as IconNewSuite,
  // Copying selected cases into another suite: a folder with an arrow in.
  FolderInput as IconCopyToSuite,

  // Undoing and stopping
  Undo2 as IconUndo,
  X as IconCancel,
  ChevronsDownUp as IconCollapseAll,
  ArrowRightLeft as IconMoveToPbi,
  Eraser as IconClear,
  Trash2 as IconRemove,
  EyeOff as IconStopWatching,
  Square as IconStop,
  Unplug as IconUnregister,
  // Wiping saved script FILES for a PBI's cases (Auto Run, dev builds only).
  FileX2 as IconClearScripts,
  // Wiping the saved run RECORDS and their screenshots (Auto Run, dev
  // builds only). `Eraser` is already IconClear for something else, so
  // this gets its own glyph rather than reusing that one.
  ListX as IconClearResults,

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
  LogIn as IconSignIn,
  RefreshCw as IconRefresh,
  Plug as IconRegister,
  Compass as IconTour,
  Bug as IconBug,
  KanbanSquare as IconBoard,
  // The tester's own test accounts, and the project's way of signing in.
  Users as IconAccounts,
  KeyRound as IconRecipe,
  // The app's own mark, for the way BACK to the test case side - the
  // mode switch names its destination, so its icon has to as well.
  FlaskConical as IconTestCases,
} from "lucide-react";
