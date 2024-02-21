#ifndef NEW_SYSCALLS_H_
#define NEW_SYSCALLS_H_

#include <ckb_syscalls.h>

typedef struct spawn2_args_t {
  /* Spawned VM instance ID */
  uint64_t *instance_id;
  /* A list of pipes, 0 indicates end of array */
  const uint64_t *pipes;
} spawn2_args_t;

// Ideally we would have only one spawn in CKB, but the previously design
// spawn is already included in ckb-c-stdlib, we have to use a different name
// here.
int ckb_spawn2(size_t index, size_t source, size_t bounds, int argc,
               char *argv[], spawn2_args_t *spgs) {
  return syscall(2601, index, source, bounds, argc, argv, spgs);
}

int ckb_join(uint64_t id, int8_t *exit_code) {
  return syscall(2602, id, exit_code, 0, 0, 0, 0);
}

uint64_t ckb_instance_id() {
  return syscall(2603, 0, 0, 0, 0, 0, 0);
}

int ckb_pipe(uint64_t fildes[2]) {
  return syscall(2604, fildes, 0, 0, 0, 0, 0);
}

int ckb_pipe_read(uint8_t *buffer, size_t *length, uint64_t filde) {
  volatile size_t l = *length;
  int ret = syscall(2606, buffer, &l, filde, 0, 0, 0);
  *length = l;
  return ret;
}

int ckb_pipe_write(const uint8_t *buffer, size_t *length, uint64_t filde) {
  volatile size_t l = *length;
  int ret = syscall(2605, buffer, &l, filde, 0, 0, 0);
  *length = l;
  return ret;
}

#endif /* NEW_SYSCALLS_H_ */
