#ifndef DG_XPANSE_STDLIB_H
#define DG_XPANSE_STDLIB_H

#include <stddef.h>

/* Freestanding stdlib.h - minimal implementation */

void *malloc(size_t size);
void *calloc(size_t nmemb, size_t size);
void *realloc(void *ptr, size_t size);
void free(void *ptr);

void *bsearch(const void *key, const void *base, size_t nmemb, size_t size,
              int (*compar)(const void *, const void *));
void qsort(void *base, size_t nmemb, size_t size,
           int (*compar)(const void *, const void *));

int abs(int x);
long labs(long x);
long long llabs(long long x);

int atoi(const char *str);
long strtol(const char *str, char **endptr, int base);
double strtod(const char *str, char **endptr);
double atof(const char *str);

int sscanf(const char *str, const char *fmt, ...);

void exit(int status);
void abort(void);
int system(const char *cmd);
char *getenv(const char *name);

void *memchr(const void *src, int c, size_t n);

#define NULL ((void *)0)

#endif
