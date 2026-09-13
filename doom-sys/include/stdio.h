#ifndef DG_XPANSE_STDIO_H
#define DG_XPANSE_STDIO_H

#include <stddef.h>
#include <stdarg.h>

/* Freestanding stdio.h - minimal implementation */

typedef struct {
    int kind;
} FILE;

extern FILE *stdin;
extern FILE *stdout;
extern FILE *stderr;

int printf(const char *fmt, ...);
int fprintf(FILE *f, const char *fmt, ...);
int sprintf(char *str, const char *fmt, ...);
int snprintf(char *str, size_t n, const char *fmt, ...);
int vprintf(const char *fmt, va_list ap);
int vfprintf(FILE *f, const char *fmt, va_list ap);
int vsnprintf(char *str, size_t n, const char *fmt, va_list ap);

int putchar(int c);
int puts(const char *s);
int fputc(int c, FILE *f);
int fputs(const char *s, FILE *f);
int fflush(FILE *f);

FILE *fopen(const char *path, const char *mode);
size_t fread(void *ptr, size_t size, size_t nmemb, FILE *stream);
size_t fwrite(const void *ptr, size_t size, size_t nmemb, FILE *stream);
int fseek(FILE *stream, long offset, int whence);
long ftell(FILE *stream);
char *fgets(char *str, int n, FILE *stream);
int fclose(FILE *stream);
int feof(FILE *stream);

int remove(const char *path);
int rename(const char *old, const char *new);

int sscanf(const char *str, const char *fmt, ...);

#define EOF (-1)
#define SEEK_SET 0
#define SEEK_CUR 1
#define SEEK_END 2

#endif
