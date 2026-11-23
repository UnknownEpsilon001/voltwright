// User-facing text and small fixed values shared by the shell and dialogs.
// Labels may contain `&` mnemonics; `dialogs::qt` strips them for egui.

// File menu.
pub const FILE_NEW: &str = "New Project";
pub const FILE_OPEN: &str = "Open Project";
pub const FILE_SAVE: &str = "Save Project";
pub const FILE_SAVE_AS: &str = "Save Project As";
pub const FILE_EXIT: &str = "Exit";

// Project files.
pub const PROJECT_EXTENSION: &str = ".volt";
pub const NEW_PROJECT_FILENAME: &str = "untitled.volt";
pub const OPEN_PROJECT_CAPTION: &str = "Open Project";
pub const SAVE_PROJECT_CAPTION: &str = "Save Project";
pub const OPEN_FILTERS: &str = "Voltwright projects (*.volt), or import a .pdsprj project";
pub const NEW_PROJECT_WIZARD_TITLE: &str = "New Project";
pub const NEW_PROJECT_WIZARD_SIZE: (u32, u32) = (640, 480);
pub const NEW_PROJECT_WIZARD_BUTTONS: [&str; 4] = ["Back", "Create", "Cancel", "Help"];

// Property Assignment tool.
pub const PROPERTY_ASSIGN_TITLE: &str = "Assign Properties";
pub const PROPERTY_ASSIGN_HINT: &str = "Use '#' in the text to insert the running count";
pub const PROPERTY_ASSIGN_COUNT_LABEL: &str = "&Count:";
pub const PROPERTY_ASSIGN_INCREMENT_LABEL: &str = "&Step:";
pub const PROPERTY_ASSIGN_STRING_LABEL: &str = "&Text:";
pub const PROPERTY_ASSIGN_OK: &str = "&Apply";
pub const PROPERTY_ASSIGN_CANCEL: &str = "&Cancel";
pub const PROPERTY_ASSIGN_APPLY_TO_TITLE: &str = "Apply to:";
pub const PROPERTY_ASSIGN_APPLY_TO: [&str; 4] =
    ["Each click", "Selected on this sheet", "Selected everywhere", "Everything"];
pub const PROPERTY_ASSIGN_ACTION_TITLE: &str = "Action:";
// Order matches the PROP_ACTION_* indexes below.
pub const PROPERTY_ASSIGN_ACTIONS: [&str; 6] =
    ["Set", "Remove", "Rename", "Show", "Hide", "Resize"];
pub const PROPERTY_ASSIGN_HELP_TITLE: &str = "Common properties";
pub const PROPERTY_ASSIGN_HELP_LABELS: [&str; 5] = [
    "Part: REF, VALUE, DEVICE",
    "Pin: NAME, NUM, TYPE",
    "Terminal: NET, TYPE",
    "Port: REF",
    "Write NAME=VALUE; wrap the value in braces to hide it",
];
pub const PROPERTY_ASSIGN_DEFAULT_STRING: &str = "NAME=VALUE";
pub const PROPERTY_ASSIGN_DEFAULT_COUNT: i32 = 0;
pub const PROPERTY_ASSIGN_DEFAULT_INCREMENT: i32 = 1;
/// Largest text height or width accepted by Resize, in schematic units.
pub const PROPERTY_ASSIGN_MAX_DIM: i32 = 2540;
// Visibility flags carried with an assigned property.
pub const PROPERTY_ASSIGN_FLAG_DEFAULT: u16 = 4;
pub const PROPERTY_ASSIGN_FLAG_FIRST_BRACE: u16 = 1;
pub const PROPERTY_ASSIGN_FLAG_EQUALS_BRACE: u16 = 2;
pub const PROP_ACTION_ASSIGN: usize = 0;
pub const PROP_ACTION_REMOVE: usize = 1;
pub const PROP_ACTION_RENAME: usize = 2;
pub const PROP_ACTION_SHOW: usize = 3;
pub const PROP_ACTION_HIDE: usize = 4;
pub const PROP_ACTION_RESIZE: usize = 5;

// Search and Tag tool.
pub const SEARCH_TAG_TITLE: &str = "Find and Select";
pub const SEARCH_TAG_PROPERTY_LABEL: &str = "&Property:";
pub const SEARCH_TAG_STRING_LABEL: &str = "&Text:";
pub const SEARCH_TAG_BUTTON: &str = "&Find";
pub const SEARCH_TAG_MODE_TITLE: &str = "Results:";
pub const SEARCH_TAG_MODE_NEW: &str = "Replace selection";
pub const SEARCH_TAG_MODE_OR: &str = "Add to selection";
pub const SEARCH_TAG_MODE_AND: &str = "Narrow selection";
pub const SEARCH_TAG_MATCH_TITLE: &str = "Match:";
// Order matches the MatchMode list in dialogs.rs.
pub const SEARCH_TAG_MATCH_MODES: [&str; 6] = [
    "Does not start with",
    "Is not",
    "Contains",
    "Does not contain",
    "Starts with",
    "Is",
];
pub const SEARCH_TAG_WHOLE_DESIGN: &str = "Search all sheets";
pub const SEARCH_TAG_CASE_SENSITIVE: &str = "Match case";
pub const SEARCH_TAG_DONE: &str = "&Done";
pub const SEARCH_TAG_CANCEL: &str = "&Cancel";
/// First `%d` is the match count, second is the selection size.
pub const SEARCH_TAG_RESULT_FORMAT: &str = "%d match(es), %d selected.";
