# Moves

Ownership is the rule the rest of Rust leans on. Follow one value from creation to drop and most of the compiler's complaints start to make sense.

Passing a value to a function moves it into the parameter, so the caller loses access unless the function hands it back.

## One owner at a time

Every value has exactly one owner, and the value is dropped when that owner goes out of scope.

Bind a String to a second name and the first name stops working. The compiler treats that as a move, not a duplicate.

```rust
let a = String::from("hi");
let b = a;
```

## What actually moves

Think of a String as a small header pointing at a buffer. A move copies only the pointer, length and capacity, and leaves the heap bytes where they are.

Two headers pointing at one buffer would free it twice, which is the bug that invalidating the old name prevents.

## Types that copy instead

Integers, booleans and other types that live entirely on the stack implement Copy, so assigning them leaves the source usable.

Copy and a destructor cannot coexist on one type. A bitwise copy of a value that needs cleanup would clean up twice.

## Handing a value back

A function can build a value and return it, which moves ownership to the caller without any copy of the heap data.

When the caller needs the value again after a call, that return trip is the cheapest way to get it.
