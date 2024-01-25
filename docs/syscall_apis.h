typedef struct spawn_args_t {
  /* Spawned VM instance ID */
  uint64_t *instance_id;
  /* A list of pipes, 0 indicates end of array */
  const uint64_t *pipes[];
} spawn_args_t;

/*
 * Spawn a new VM instance, with an optional number of pipes given
 * to the spawned VM instance.
 * An id for the spawned VM instance is returned.
 */
int ckb_spawn(size_t index, size_t source, size_t bounds, int argc,
              char *argv[], spawn_args_t *spgs);
/*
 * Given an id for a spawned VM instance, block till the specified
 * VM instance terminates, and fetches its exit code.
 */
int ckb_join(uint64_t id, int8_t *exit_code);
/*
 * Get current VM instance ID.
 */
uint64_t ckb_instance_id();

/*
 * Create a pair of pipes owned by current VM instance. All pipe might
 * have non-zero value as filde.
 */
int ckb_pipe(uint64_t fildes[2]);
/*
 * Blocking read from a pipe, might read less data than the buffer size
 */
int ckb_pipe_read(uint8_t *buffer, size_t *length, uint64_t filde);
/*
 * Blocking write to a pipe, might write less data from the buffer to the pipe
 */
int ckb_pipe_write(const uint8_t *buffer, size_t *length, uint64_t filde);
