#ifndef DG_H_setjmp
#define DG_H_setjmp
typedef int jmp_buf[28];
int setjmp(jmp_buf env);
void longjmp(jmp_buf env, int val) __attribute__((noreturn));
#endif
