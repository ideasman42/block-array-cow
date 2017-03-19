
###############
Block Array Cow
###############

Introduction
============

In memory array de-duplication, useful for efficiently storing many versions of data.

This is suitable for storing undo history for example - where the size of a struct can be used as the stride,
and is effective with both binary and text data.

This has a slight emphasis on performance, since this method is used in Blender's undo system.
Where making users of the application wait for an exhaustive method isn't acceptable.
So hashed memory blocks are used to detect duplicates.

The code is Apache2.0 licensed and doesn't have any dependencies.


Supported
---------

- Caller defined block sizes.
- Caller defined array-stride to avoids overhead of detecting possible matches it un-aligned offsets.
  *(a stride of 1 for bytes works too)*
- De-duplication even in the case blocks are completely re-ordered
  *(block hashing is used for de-duplication)*.
- Each state only needs to reference its previous,
  making both linear and tree structures possible.
- Out of order adding/freeing states.


Unsupported
-----------

In general operations that would use excessive calculation are avoided,
since there are many possible changes that would improve memory usage at the cost of performance.

- Re-aligning of single-user reference block boundaries
  to reduce the size of duplicate blocks when changes are found.
- Detecting numeric changes to the data (values incremented/decremented, zeroed etc... are not detected).
- Reversing data.


Further Work
============

Some things that may be worth considering.

- It may be worth using ``mmap`` for data storage.
- Block compression
  *(likely based on caller defined rule about when a state's data isn't likely to be read again).*


Links
=====

- `Crates.io <https://crates.io/crates/block-array-cow>`__.
- `API docs <https://docs.rs/block-array-cow>`__.
