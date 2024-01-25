Pipes can be created using the following API:

```
int pipe(size_t fildes[2]);
```

Upon success completion, 0 shall be returned. `fildes` will be filled with 2 **non-zero** values from syscalls. `fildes[0]` can be used to read from the pipe, while `fildes[1]` can be used to write to the pipe.

Only current VM instance can read from / write to the created pipes for now.

`spawn` shall be modified so `spawn_args_t` is like the following:

```
typedef struct spawn_args_t {
  uint64_t *instance_id;
  const uint64_t *pipes[];
} spawn_args_t;
```
The first 0 zero in `pipes` indicates the end of an array.

The passed pipes must be **owned** by the parent, upon success invoking of `spawn` syscall, the ownership of a pipe will be transferred from parent to child. And a parent might not read from / write to a pipe that it has transferred to the child.

A VM instance can **own** a pipe by:

* Creating the pipe itself
* Inherit a pipe via spawn syscall from the parent.

A pair of read / write methods will be provided to access pipes:

```
int pipe_read(uint8_t *buffer, size_t* length, size_t filde);
int pipe_write(const uint8_t *buffer, size_t length, size_t filde);
```

pipe_read will block until data is available, `length` is used both as input and output. The returned `length` might be smaller than the original `length`(indicating buffer length).

pipe_write will block until data can be written to the other side. In typical cases, pipe_write will return when all its data has been written to the other side, but in case the other end of the pipe is closed, pipe_write would return with less data written than provided.

As a result, programs can consider the other end of the pipe to be caused, when:

* Part of the data has been written to the other side
* 0 bytes of data has been written to the other side

For both read & write, if the other side of the pipe has been closed before starting a read / write operation, a designated error code will be returned. However if the other side of the pipe is closed while performing a read / write operation, a SUCCESS error code will be returned, but `length` will indicate that less data than the provided buffer has been read / written.

Terminating a VM instance will close all its pipes, we can also choose to provide a designated syscall for closing a pipe if needed.

`spawn` syscall also returns an `instance id` for joining operations:

```
typedef struct spawn_args_t {
  uint64_t *instance_id;
  const uint64_t *pipes[];
} spawn_args_t;

int ckb_spawn(size_t index, size_t source, size_t bounds, int argc,
              char *argv[], spawn_args_t *spgs);
int ckb_join(uint64_t id, int8_t *exit_code);
uint64_t ckb_instance_id();
```

Each VM is also getting a unique ID among all spawned VM instances.

All spawned VMs are considered to be paralleled to each other, there is no hierarchies to maintain. It's perfectly fine to do this:

* VM 0 spawned VM 1 and VM 2, then pass VM 1's ID to VM 2 via a pipe
* VM 2 then join till VM1 finishes execution.
