;; A program built against `wasdk:platform@2.0.0`, the release before `service-extensions` came in
;; `2.2.0`: it imports `wasdk:platform/system` by that version, as such a program does, with the
;; shape both versions share, so the engine, which serves `2.2.0`, links it by semver's rule. It
;; imports and does nothing else, and has no `wasi:cli/run`, so it fails after it is linked, for
;; that, never as incompatible. Text, which wasmtime compiles as it does a component's binary.
(component
  (import "wasdk:platform/system@2.0.0" (instance
    (type (enum "android" "fuchsia" "ios" "linux" "macos" "windows" "other"))
    (export "operating-system" (type (eq 0)))
    (type (func (result 1)))
    (export "os" (func (type 2)))
  ))
)
