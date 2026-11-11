# Moves

Assigning a `String` to a new binding moves it. The old binding can no longer be used.

```rust
let a = String::from("hi");
let b = a;
```
