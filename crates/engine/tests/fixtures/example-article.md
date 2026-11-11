# Ownership in Rust

Every value has one owner. When the owner goes out of scope, the value is dropped.

## Borrowing

A reference borrows a value without taking ownership. You can have many shared
references or one mutable reference, never both at once.

- Move: ownership transfers to a new binding.
- Borrow: a reference reads or changes the value for a while.

Source note in Thai: ภาษาไทยต้องอ่านได้ครบ

See the [reference](https://doc.rust-lang.org/book/ch04-00-understanding-ownership.html).
