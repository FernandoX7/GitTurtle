; GitTurtle patch: tree-sitter-diff 0.1.0's highlights with the file
; headers, hunk locations and changed lines under names of their own, so a
; theme can draw them in its patch colours instead of string, keyword and
; attribute, and context lines, which upstream leaves uncaptured, so that a
; diff injected into Markdown does not draw them as the fence's code.

[(old_file) (new_file)] @diff.header
(context) @diff.context
(deletion) @diff.minus
(addition) @diff.plus

(commit) @constant
(location) @diff.hunk
(command) @variable.builtin
