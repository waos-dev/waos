;; A program that asks for the inspector role: it imports an interface of `wasdk:inspect`, which a
;; component imports only when it is to hold the role. It imports and does nothing else, since what
;; is tested happens before anything of a program runs: its roles are read from its imports and
;; checked before it is linked. Text, which wasmtime compiles as it does a component's binary.
(component
  (import "wasdk:inspect/devices@1.0.0" (instance
    (export "device" (type (sub resource)))
  ))
)
