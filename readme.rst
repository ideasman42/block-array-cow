
#########
Array Cow
#########

Introduction
============

In memory array de-duplication, useful for efficiently storing many versions of data.

This is suitable for storing undo history for example - where the size of a struct can be used as the stride,
and is effective with both binary and text data.


Supported
---------

- Configurable block sizes.
- Supports array-stride to avoids overhead of detecting blocks and un-aliened offsets.
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
