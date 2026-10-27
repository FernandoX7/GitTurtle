; GitTurtle patch: tree-sitter-diff 0.1.0's highlights with the file
; headers, hunk locations and changed lines under names of their own, so a
; theme can draw them in its patch colours instead of string, keyword and
; attribute.

[(old_file) (new_file)] @diff.header
(deletion) @diff.minus
(addition) @diff.plus

(commit) @constant
(location) @diff.hunk
(command) @variable.builtin
