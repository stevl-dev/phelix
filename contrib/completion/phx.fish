#!/usr/bin/env fish
# Fish completion script for Helix editor

complete -c phx -s h -l help -d "Prints help information"
complete -c phx -l strict -d "Bail on error for commands that can fail"
complete -c phx -l tutor -d "Loads the tutorial"
complete -c phx -l health -xa "(__phx_langs_ops)" -d "Checks for errors"
complete -c phx -l health -xka all -d "Prints all diagnostic informations"
complete -c phx -l health -xka all-languages -d "Lists all languages"
complete -c phx -l health -xka languages -d "Lists user configured languages"
complete -c phx -l health -xka clipboard -d "Prints system clipboard provider"
complete -c phx -s g -l grammar -x -a "fetch build" -d "Fetch or build tree-sitter grammars"
complete -c phx -s v -o vv -o vvv -d "Increases logging verbosity"
complete -c phx -s V -l version -d "Prints version information"
complete -c phx -l vsplit -d "Splits all given files vertically"
complete -c phx -l hsplit -d "Splits all given files horizontally"
complete -c phx -s c -l config -r -d "Specifies a file to use for config"
complete -c phx -l log -r -d "Specifies a file to use for logging"
complete -c phx -s w -l working-dir -d "Specify initial working directory" -xa "(__fish_complete_directories)"

function __phx_langs_ops
    phx --health all-languages | tail -n '+2' | string replace -fr '^(\S+) .*' '$1'
end
