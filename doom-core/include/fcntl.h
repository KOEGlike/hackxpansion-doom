#ifndef DG_H_fcntl
#define DG_H_fcntl
#define O_RDONLY 0
int open(const char *path, int flags, ...);
int close(int fd);
int read(int fd, void *dst, unsigned int n);
int write(int fd, const void *src, unsigned int n);
#endif
