/*
 * The variadic functions of the Max C API. Rust cannot define these on stable, so each one
 * collects its variable arguments here and forwards them to a non-variadic Rust function (the
 * `median_mock_*` functions in src/). The signatures match the Max SDK headers; no SDK headers are
 * needed, so types are spelled with their ABI equivalents.
 */
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>

/* Most arguments a class_new/class_addmethod type list can carry (MSG_MAXARG in the SDK). */
#define MOCK_MAX_TYPES 32

/* Rust side, see src/class.rs, src/console.rs */
extern void *median_mock_class_new(const char *name, void *mnew, void *mfree, long size,
                                   void *mmenu, const int *types, int ntypes);
extern long median_mock_class_addmethod(void *c, void *m, const char *name, const int *types,
                                        int ntypes);
extern void median_mock_console(int level, void *obj, const char *text);
extern void *median_mock_object_method(void *x, void *s, void *a0, void *a1, void *a2, void *a3);

/* The type list of class_new is `type, ...` terminated by a 0 after the last type; `type` itself
 * is the first entry (0 meaning no arguments).
 *
 * The SDK header declares `type` as `short`, but va_start on a parameter that undergoes default
 * argument promotion is undefined behaviour (-Wvarargs). It is declared `int` here instead: on the
 * supported ABIs (x86-64 and arm64 on macOS, x64 on Windows) a short argument occupies a full
 * 32-bit register slot extended by the caller, so the definition is call-compatible with the
 * header's declaration, and the value is narrowed back below. */
void *class_new(const char *name, void *mnew, void *mfree, long size, void *mmenu, int type,
                ...) {
    int types[MOCK_MAX_TYPES];
    int n = 0;
    va_list ap;
    int t = (short)type;
    va_start(ap, type);
    while (t != 0 && n < MOCK_MAX_TYPES) {
        types[n++] = t;
        t = va_arg(ap, int);
    }
    va_end(ap);
    return median_mock_class_new(name, mnew, mfree, size, mmenu, types, n);
}

long class_addmethod(void *c, void *m, const char *name, ...) {
    int types[MOCK_MAX_TYPES];
    int n = 0;
    va_list ap;
    int t;
    va_start(ap, name);
    t = va_arg(ap, int);
    while (t != 0 && n < MOCK_MAX_TYPES) {
        types[n++] = t;
        t = va_arg(ap, int);
    }
    va_end(ap);
    return median_mock_class_addmethod(c, m, name, types, n);
}

/* Forwards up to four pointer sized arguments; see median_mock_object_method. */
void *object_method(void *x, void *s, ...) {
    void *a[4];
    int i;
    va_list ap;
    va_start(ap, s);
    for (i = 0; i < 4; i++) {
        a[i] = va_arg(ap, void *);
    }
    va_end(ap);
    return median_mock_object_method(x, s, a[0], a[1], a[2], a[3]);
}

static void emit(int level, void *obj, const char *fmt, va_list ap) {
    char stack[1024];
    char *buf;
    int n;
    va_list ap2;
    if (fmt == NULL) {
        fmt = "";
    }
    va_copy(ap2, ap);
    n = vsnprintf(stack, sizeof stack, fmt, ap);
    if (n < 0) {
        median_mock_console(level, obj, "");
    } else if ((size_t)n < sizeof stack) {
        median_mock_console(level, obj, stack);
    } else {
        buf = (char *)malloc((size_t)n + 1);
        if (buf != NULL) {
            vsnprintf(buf, (size_t)n + 1, fmt, ap2);
            median_mock_console(level, obj, buf);
            free(buf);
        } else {
            median_mock_console(level, obj, stack);
        }
    }
    va_end(ap2);
}

/* levels: 0 post, 1 warn, 2 error; must match console::Level */
void object_post(void *x, const char *fmt, ...) {
    va_list ap;
    va_start(ap, fmt);
    emit(0, x, fmt, ap);
    va_end(ap);
}

void object_warn(void *x, const char *fmt, ...) {
    va_list ap;
    va_start(ap, fmt);
    emit(1, x, fmt, ap);
    va_end(ap);
}

void object_error(void *x, const char *fmt, ...) {
    va_list ap;
    va_start(ap, fmt);
    emit(2, x, fmt, ap);
    va_end(ap);
}

void post(const char *fmt, ...) {
    va_list ap;
    va_start(ap, fmt);
    emit(0, NULL, fmt, ap);
    va_end(ap);
}

void error(const char *fmt, ...) {
    va_list ap;
    va_start(ap, fmt);
    emit(2, NULL, fmt, ap);
    va_end(ap);
}
