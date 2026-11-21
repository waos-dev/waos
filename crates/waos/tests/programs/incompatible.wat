;; A program built for another version of the interface: it imports `wasdk:platform/system` as it
;; would have been before `operating-system` had its `other` case, so the engine, which serves the
;; enum with it, cannot link it. It imports and does nothing else: the link fails before anything
;; of a program runs, its exports with it. Text, which wasmtime compiles as it does a component's
;; binary.
(component
  (import "wasdk:platform/system@1.0.0" (instance
    (type (enum "android" "fuchsia" "ios" "linux" "macos" "windows"))
    (export "operating-system" (type (eq 0)))
    (type (func (result 1)))
    (export "os" (func (type 2)))
  ))
)
