# Moves

Rust tracks who owns each value, and that one idea explains most of its rules. This chapter follows a single value as it changes hands.

## One owner at a time

Every value has exactly one owner, and the value is dropped when that owner goes out of scope.

Assigning a String to a new binding moves it. The old binding cannot be used afterwards.

```rust
let a = String::from("hi");
let b = a;
```

## Why a move is not a copy

A move copies only the pointer, length and capacity, and leaves the heap bytes where they are.

Two owners of one buffer would free it twice. Invalidating the old binding rules that out at compile time.

## Types that copy instead

Integers, booleans and other types that live entirely on the stack implement Copy, so assigning them leaves the source usable.

A type with a destructor cannot also be Copy, because a bitwise copy would run the destructor twice.

## Moves into functions

Passing a value to a function moves it into the parameter, so the caller loses access unless the function hands it back.

Returning a value moves ownership out to the caller, which is how a function can build something and give it away.

## Cloning when you need two

Call clone when you really want two independent values, and accept that it copies the heap data.

Cloning is explicit on purpose, so the cost shows at the call site.
