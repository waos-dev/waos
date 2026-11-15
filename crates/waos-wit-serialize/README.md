# waos-wit-serialize

`#[derive(WitSerialize)]`: serde's `Serialize` for the types wasmtime's `bindgen!` makes of a WIT package, by the WIT's own names, which the bindgen writes on each field and case as `#[component(name = "…")]`. JSON of a value then reads as the WIT says it: a record an object of its fields (`dropped-before`), a case its name (`stdout`), or an object of its name holding its payload (`{"read": {"form": "text/plain", "pasted": false}}`), an option `null` or its value, a list or a tuple an array.

- **Takes:** a record, variant or enum of wasmtime's `bindgen!`, given as one of its `additional_derives`.
- **Provides:** `WitSerialize`, an implementation of `serde::Serialize`; the crate deriving needs `serde` among its dependencies.
- **Used by:** `waos`, for `wasdk:inspect`, whose values `cargo waos inspect` prints as JSON lines.

```rust
wasmtime::component::bindgen!({
    path: "wit",
    world: "wasdk:inspect/inspection",
    additional_derives: [waos_wit_serialize::WitSerialize, PartialEq],
});
```
