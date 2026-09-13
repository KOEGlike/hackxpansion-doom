#ifndef DG_H_strings
#define DG_H_strings

/* Freestanding strings.h for strcasecmp/strncasecmp */
#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

int strcasecmp(const char *a, const char *b);
int strncasecmp(const char *a, const char *b, size_t n);

#ifdef __cplusplus
}
#endif

#endif
