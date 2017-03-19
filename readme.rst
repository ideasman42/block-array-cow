
###############
Block Array Cow
###############

Introduction
============

In memory array de-duplication, useful for efficiently storing many versions of data.

This is suitable for storing undo history for example - where the size of a struct can be used as the stride,
and is effective with both binary and text data.

The code is Apache2.0 licensed and doesn't have any dependencies.


Motivation
==========

For an undo system (or any other history storage) you may want to store many versions of your data.

In some cases it makes sense to write a
`persistent data structure <https://en.wikipedia.org/wiki/Persistent_data_structure>`__
but this depends a lot on the kind of data you're dealing with.

In other cases its nice to have the convenience of being able to serialize your data and store it
without worrying about the details of how duplication is managed.

Thats the motivation for writing this library.


Algorithm
=========

This has a slight emphasis on performance, since this method is used in Blender's undo system.
Where making users of the application wait for an exhaustive method isn't acceptable.

- A new ``BArrayStore`` is created with a fixed stride and block size.
- Adding a new state to the array store simply divides the array into blocks and stores them.
- Adding another state can use any previous state as a reference, where its blocks will be re-used where possible.
- Matching blocks at the start/end of the array are checked and copied until s mismatch is found.
- If a mismatch is found, the reference blocks use a lazily initialized hash of their first *N* bytes.
  A hash data for the data being added with a value for each stride offset is calculated too.

  Looping over the newly added state data can now perform hash look-ups on the reference chunks
  and then a full comparison if a match is found.

  In that case the following chunks are tested to see if they match (to avoid further lookups),
  otherwise a new chunk is allocated.
- On completion the new state is added which may contain both new and reused chunks from previous states.


Where *N* is currently the ``stride * 7``, see: ``BCHUNK_HASH_TABLE_ACCUMULATE_STEPS``.


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
