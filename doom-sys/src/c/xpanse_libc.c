/* Minimal freestanding libc for the RP2350 doom port.
 * Memory functions come from Rust's compiler_builtins via DG_PLATFORM v-table. */
#include <string.h>
#include <ctype.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <errno.h>
#include "config.h"
#include "doomtype.h"
#include "doomgeneric.h"

extern const struct DgPlatform DG_PLATFORM;
static const struct DgPlatform *platform = &DG_PLATFORM;
extern void dg_fatal(const char *msg);

// --- heap (backed by the Rust allocator) ------------------------------------

void *malloc(unsigned int size)
{
    return platform->heap_alloc(size);
}

void *calloc(unsigned int n, unsigned int size)
{
    unsigned long total = (unsigned long)n * size;
    return platform->heap_alloc_zeroed((unsigned int)total);
}

void *realloc(void *ptr, unsigned int size)
{
    if (ptr == NULL)
        return malloc(size);
    if (size == 0)
    {
        free(ptr);
        return NULL;
    }
    unsigned int old = platform->heap_alloc_size(ptr);
    void *newp = malloc(size);
    if (newp == NULL)
        return NULL;
    if (old > size)
        old = size;
    memcpy(newp, ptr, old);
    free(ptr);
    return newp;
}

void free(void *ptr)
{
    if (ptr == NULL)
        return;
    platform->heap_free(ptr, platform->heap_alloc_size(ptr));
}

// --- string -----------------------------------------------------------------

int errno = 0;

int strcmp(const char *a, const char *b)
{
    while (*a != 0 && *a == *b)
    {
        a++;
        b++;
    }
    return (unsigned char)*a - (unsigned char)*b;
}

char *strdup(const char *s)
{
    unsigned int n = strlen(s) + 1;
    char *p = malloc(n);
    if (p != NULL)
        memcpy(p, s, n);
    return p;
}

int strcasecmp(const char *a, const char *b)
{
    while (*a != 0 && *b != 0)
    {
        int ca = toupper((unsigned char)*a);
        int cb = toupper((unsigned char)*b);
        if (ca != cb)
            return ca - cb;
        a++;
        b++;
    }
    return toupper((unsigned char)*a) - toupper((unsigned char)*b);
}

int strncasecmp(const char *a, const char *b, unsigned int n)
{
    for (; n > 0; a++, b++, n--)
    {
        int ca = toupper((unsigned char)*a);
        int cb = toupper((unsigned char)*b);
        if (ca != cb)
            return ca - cb;
        if (ca == 0)
            return 0;
    }
    return 0;
}

char *strncpy(char *dst, const char *src, unsigned int n)
{
    unsigned int i = 0;
    while (i < n && src[i] != 0)
    {
        dst[i] = src[i];
        i++;
    }
    while (i < n)
        dst[i++] = 0;
    return dst;
}

// --- ctype ------------------------------------------------------------------

int tolower(int c)
{
    if (c >= 'A' && c <= 'Z')
        return c - 'A' + 'a';
    return c;
}

int toupper(int c)
{
    if (c >= 'a' && c <= 'z')
        return c - 'a' + 'A';
    return c;
}

int isalpha(int c)
{
    return (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z');
}

int isdigit(int c)
{
    return c >= '0' && c <= '9';
}

int isspace(int c)
{
    return c == ' ' || (c >= 9 && c <= 13);
}

int isupper(int c)
{
    return c >= 'A' && c <= 'Z';
}

int islower(int c)
{
    return c >= 'a' && c <= 'z';
}

int isprint(int c)
{
    return c >= 32 && c < 127;
}

int isxdigit(int c)
{
    return isdigit(c) || (c >= 'a' && c <= 'f') || (c >= 'A' && c <= 'F');
}

// --- numbers ----------------------------------------------------------------

int atoi(const char *s)
{
    return (int)strtol(s, NULL, 10);
}

long strtol(const char *s, char **end, int base)
{
    long result = 0;
    int negative = 0;
    while (*s == ' ' || (*s >= 9 && *s <= 13))
        s++;
    if (*s == '-')
    {
        negative = 1;
        s++;
    }
    else if (*s == '+')
        s++;
    if ((base == 0 || base == 16) && s[0] == '0' && (s[1] == 'x' || s[1] == 'X'))
    {
        base = 16;
        s += 2;
    }
    else if (base == 0)
    {
        base = s[0] == '0' ? 8 : 10;
    }
    for (;; s++)
    {
        int digit;
        if (*s >= '0' && *s <= '9')
            digit = *s - '0';
        else if (*s >= 'a' && *s <= 'z')
            digit = *s - 'a' + 10;
        else if (*s >= 'A' && *s <= 'Z')
            digit = *s - 'A' + 10;
        else
            break;
        if (digit >= base)
            break;
        result = result * base + digit;
    }
    if (end != NULL)
        *end = (char *)s;
    return negative ? -result : result;
}

/* Integer-only strtod stub. The Cortex-M33 FPU has no double precision,
 * so real double arithmetic would emit VFP instructions that fault.
 * No linked console code path needs fractional config values (joystick
 * binds are integers, timing-demo FPS is never shown), so parse the
 * leading integer and return it as double via an integer->double
 * conversion (soft __aeabi_i2d call, no VFP). */
double strtod(const char *s, char **end)
{
    int negative = 0;
    int value = 0;
    while (*s == ' ')
        s++;
    if (*s == '-')
    {
        negative = 1;
        s++;
    }
    else if (*s == '+')
        s++;
    while (*s >= '0' && *s <= '9')
        value = value * 10 + (*s++ - '0');
    /* Skip any fraction/exponent text so endptr still advances. */
    if (*s == '.')
    {
        s++;
        while (*s >= '0' && *s <= '9')
            s++;
    }
    if (*s == 'e' || *s == 'E')
    {
        const char *p = s + 1;
        if (*p == '-' || *p == '+')
            p++;
        if (*p >= '0' && *p <= '9')
        {
            s = p;
            while (*s >= '0' && *s <= '9')
                s++;
        }
    }
    if (end != NULL)
        *end = (char *)s;
    if (negative)
        value = -value;
    return (double)value;
}

double atof(const char *s)
{
    return strtod(s, NULL);
}

int sscanf(const char *s, const char *fmt, ...)
{
    /* Integer-only parser. Keeping float handling out avoids VFP opcodes on
     * RP235x startup paths where config helpers may parse numeric strings. */
    va_list ap;
    int assigned = 0;
    va_start(ap, fmt);
    for (;;)
    {
        if (*fmt == '%')
        {
            fmt++;
            if (*fmt == 'd' || *fmt == 'i' || *fmt == 'u' || *fmt == 'x' || *fmt == 'o')
            {
                int *out = va_arg(ap, int *);
                while (*s == ' ')
                    s++;
                int base = *fmt == 'x' ? 16 : (*fmt == 'o' ? 8 : 10);
                int sign = 1;
                if (*s == '-')
                {
                    sign = -1;
                    s++;
                }
                else if (*s == '+')
                    s++;

                if (*fmt == 'i' && s[0] == '0' && (s[1] == 'x' || s[1] == 'X'))
                {
                    base = 16;
                    s += 2;
                }
                else if (*fmt == 'i' && s[0] == '0')
                {
                    base = 8;
                }

                int digit;
                if (*s >= '0' && *s <= '9')
                    digit = *s - '0';
                else if (*s >= 'a' && *s <= 'f')
                    digit = *s - 'a' + 10;
                else if (*s >= 'A' && *s <= 'F')
                    digit = *s - 'A' + 10;
                else
                    break;
                if (digit >= base)
                    break;

                int value = 0;
                for (;;)
                {
                    if (*s >= '0' && *s <= '9')
                        digit = *s - '0';
                    else if (*s >= 'a' && *s <= 'f')
                        digit = *s - 'a' + 10;
                    else if (*s >= 'A' && *s <= 'F')
                        digit = *s - 'A' + 10;
                    else
                        break;
                    if (digit >= base)
                        break;
                    value = value * base + digit;
                    s++;
                }
                *out = sign * value;
                assigned++;
            }
            else
            {
                break;
            }
        }
        else if (*fmt == 0)
        {
            break;
        }
        else if (*fmt == ' ')
        {
            while (*s == ' ')
                s++;
        }
        else if (*s == *fmt)
        {
            s++;
        }
        else
        {
            break;
        }
        fmt++;
    }
    va_end(ap);
    return assigned;
}

// --- stdio ------------------------------------------------------------------

/* printf-style output goes straight to the engine log; the only files are
 * in-memory WAD handles, so stdio file calls fail. */
struct dg_file
{
    int kind;
};

static struct dg_file dg_std_file;
static struct dg_file dg_err_file;

FILE *stdout = (FILE *)&dg_std_file;
FILE *stderr = (FILE *)&dg_err_file;

static void dg_emit(char c, void *ctx)
{
    (void)ctx;
    char c2 = c;
    platform->log_str(&c2, 1);
}

static void dg_pad(void (*emit)(char, void *), void *ctx, int width, int pad_zero, int left_align)
{
    if (left_align)
        return;
    while (width-- > 0)
        emit(pad_zero ? '0' : ' ', ctx);
}

/* Minimal printf formatter: %d %i %u %x %X %c %s %p with -, 0, width and
 * .precision — everything the vendored sources format. */
static void dg_vformat(void (*emit)(char c, void *ctx), void *ctx, const char *fmt, va_list ap)
{
    while (*fmt != 0)
    {
        if (*fmt != '%')
        {
            emit(*fmt++, ctx);
            continue;
        }
        fmt++;
        int pad_zero = 0;
        int left_align = 0;
        for (;;)
        {
            if (*fmt == '0')
                pad_zero = 1;
            else if (*fmt == '-')
                left_align = 1;
            else if (*fmt == ' ' || *fmt == '+' || *fmt == '#')
                ;
            else
                break;
            fmt++;
        }
        int width = 0;
        while (isdigit((unsigned char)*fmt))
        {
            width = width * 10 + (*fmt - '0');
            fmt++;
        }
        int precision = -1;
        if (*fmt == '.')
        {
            precision = 0;
            fmt++;
            while (isdigit((unsigned char)*fmt))
            {
                precision = precision * 10 + (*fmt - '0');
                fmt++;
            }
        }
        while (*fmt == 'l' || *fmt == 'h' || *fmt == 'z')
            fmt++;
        char spec = *fmt++;
        if (spec == 'd' || spec == 'i')
        {
            int value = va_arg(ap, int);
            unsigned int magnitude = value < 0 ? (unsigned int)(-value) : (unsigned int)value;
            char digits[12];
            int n = 0;
            do
            {
                digits[n++] = (char)('0' + magnitude % 10);
                magnitude /= 10;
            } while (magnitude != 0);
            int len = n + (value < 0 ? 1 : 0);
            if (!left_align)
                while (width-- > len)
                    emit(pad_zero ? '0' : ' ', ctx);
            if (value < 0)
                emit('-', ctx);
            while (n > 0)
                emit(digits[--n], ctx);
            while (left_align && width-- > len)
                emit(' ', ctx);
        }
        else if (spec == 'u')
        {
            unsigned int value = va_arg(ap, unsigned int);
            char digits[12];
            int n = 0;
            do
            {
                digits[n++] = (char)('0' + value % 10);
                value /= 10;
            } while (value != 0);
            if (!left_align)
                while (width-- > n)
                    emit(pad_zero ? '0' : ' ', ctx);
            while (n > 0)
                emit(digits[--n], ctx);
            while (left_align && width-- > 0)
                emit(' ', ctx);
        }
        else if (spec == 'x' || spec == 'X')
        {
            unsigned int value = va_arg(ap, unsigned int);
            char digits[10];
            int n = 0;
            do
            {
                unsigned int digit = value & 0xf;
                digits[n++] = (char)(digit < 10 ? '0' + digit : (spec == 'X' ? 'A' : 'a') + digit - 10);
                value >>= 4;
            } while (value != 0);
            if (!left_align)
                while (width-- > n)
                    emit(pad_zero ? '0' : ' ', ctx);
            while (n > 0)
                emit(digits[--n], ctx);
            while (left_align && width-- > 0)
                emit(' ', ctx);
        }
        else if (spec == 'c')
        {
            if (!left_align)
                while (width-- > 1)
                    emit(' ', ctx);
            emit((char)va_arg(ap, int), ctx);
        }
        else if (spec == 's')
        {
            const char *s = va_arg(ap, const char *);
            if (s == NULL)
                s = "(null)";
            int len = 0;
            while (s[len] != 0)
                len++;
            if (precision >= 0 && len > precision)
                len = precision;
            int padding = width - len;
            if (!left_align)
                while (padding-- > 0)
                    emit(' ', ctx);
            for (int i = 0; i < len; i++)
                emit(s[i], ctx);
            while (padding-- > 0)
                emit(' ', ctx);
        }
        else if (spec == 'p')
        {
            unsigned long value = (unsigned long)va_arg(ap, void *);
            emit('0', ctx);
            emit('x', ctx);
            char digits[10];
            int n = 0;
            do
            {
                unsigned int digit = value & 0xf;
                digits[n++] = (char)(digit < 10 ? '0' + digit : 'a' + digit - 10);
                value >>= 4;
            } while (value != 0);
            while (n > 0)
                emit(digits[--n], ctx);
        }
        else if (spec == 'f' || spec == 'g')
        {
            /* Consume the 8-byte double without touching the `double`
             * type (which would pull VFP into this formatter). */
            (void)va_arg(ap, unsigned long long);
            const char *unsupported = "<float>";
            while (*unsupported != 0)
                emit(*unsupported++, ctx);
        }
        else if (spec == '%')
        {
            emit('%', ctx);
        }
        else if (spec == 0)
        {
            break;
        }
        else
        {
            emit(spec, ctx);
        }
    }
}

int vprintf(const char *fmt, va_list ap)
{
    dg_vformat(dg_emit, NULL, fmt, ap);
    return 0;
}

int printf(const char *fmt, ...)
{
    va_list ap;
    va_start(ap, fmt);
    vprintf(fmt, ap);
    va_end(ap);
    return 0;
}

int vfprintf(FILE *f, const char *fmt, va_list ap)
{
    (void)f;
    return vprintf(fmt, ap);
}

int fprintf(FILE *f, const char *fmt, ...)
{
    va_list ap;
    va_start(ap, fmt);
    vfprintf(f, fmt, ap);
    va_end(ap);
    return 0;
}

struct dg_buf
{
    char *data;
    unsigned int capacity;
    unsigned int used;
};

static void dg_buf_emit(char c, void *ctx)
{
    struct dg_buf *buf = ctx;
    if (buf->used + 1 < buf->capacity)
        buf->data[buf->used++] = c;
}

static void dg_buf_finish(struct dg_buf *buf)
{
    if (buf->capacity > 0)
        buf->data[buf->used < buf->capacity ? buf->used : buf->capacity - 1] = 0;
}

int vsnprintf(char *out, unsigned int n, const char *fmt, va_list ap)
{
    struct dg_buf buf = {out, n, 0};
    dg_vformat(dg_buf_emit, &buf, fmt, ap);
    dg_buf_finish(&buf);
    return (int)buf.used;
}

int snprintf(char *out, unsigned int n, const char *fmt, ...)
{
    va_list ap;
    va_start(ap, fmt);
    int result = vsnprintf(out, n, fmt, ap);
    va_end(ap);
    return result;
}

int sprintf(char *out, const char *fmt, ...)
{
    va_list ap;
    va_start(ap, fmt);
    int n = vsnprintf(out, 32768, fmt, ap);
    va_end(ap);
    return n;
}

int puts(const char *s)
{
    while (*s)
        platform->log_str((const unsigned char *)s++, 1);
    platform->log_str("\n", 1);
    return 1;
}

int putchar(int c)
{
    char ch = (char)c;
    platform->log_str(&ch, 1);
    return c;
}

int putc(int c, FILE *f)
{
    (void)f;
    return putchar(c);
}

int fputc(int c, FILE *f)
{
    (void)f;
    return putchar(c);
}

int fputs(const char *s, FILE *f)
{
    (void)f;
    while (*s)
        platform->log_str((const unsigned char *)s++, 1);
    return 0;
}

int fflush(FILE *f)
{
    (void)f;
    return 0;
}

FILE *fopen(const char *path, const char *mode)
{
    (void)path;
    (void)mode;
    errno = 2; // ENOENT
    return NULL;
}

unsigned int fread(void *dst, unsigned int size, unsigned int n, FILE *f)
{
    (void)dst;
    (void)size;
    (void)n;
    (void)f;
    return 0;
}

unsigned int fwrite(const void *src, unsigned int size, unsigned int n, FILE *f)
{
    (void)src;
    (void)size;
    (void)n;
    (void)f;
    return 0;
}

int fseek(FILE *f, long ofs, int whence)
{
    (void)f;
    (void)ofs;
    (void)whence;
    return -1;
}

long ftell(FILE *f)
{
    (void)f;
    return 0;
}

char *fgets(char *dst, int n, FILE *f)
{
    (void)dst;
    (void)n;
    (void)f;
    return NULL;
}

int fclose(FILE *f)
{
    (void)f;
    return 0;
}

int feof(FILE *f)
{
    (void)f;
    return 1;
}

// --- misc -------------------------------------------------------------------

void exit(int status)
{
    (void)status;
    dg_fatal("exit called");
    for (;;)
    {
    }
}

void abort(void)
{
    dg_fatal("abort called");
    for (;;)
    {
    }
}

int system(const char *cmd)
{
    (void)cmd;
    return -1;
}

int remove(const char *path)
{
    (void)path;
    return -1;
}

int rename(const char *from, const char *to)
{
    (void)from;
    (void)to;
    return -1;
}

char *getenv(const char *name)
{
    (void)name;
    return NULL;
}

int abs(int x)
{
    return x < 0 ? -x : x;
}

int mkdir(const char *path, unsigned int mode)
{
    (void)path;
    (void)mode;
    errno = 13; // EACCES
    return -1;
}

void *memchr(const void *src, int c, unsigned int n)
{
    const unsigned char *p = src;
    for (unsigned int i = 0; i < n; i++)
    {
        if (p[i] == (unsigned char)c)
            return (void *)&p[i];
    }
    return NULL;
}
