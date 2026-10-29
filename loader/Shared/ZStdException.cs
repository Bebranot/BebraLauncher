using System;
using System.Runtime.InteropServices;
using static SharpZstd.Interop.Zstd;

namespace SS14.Launcher.Utility;

/// <summary>
/// Only the zstd error helper is kept from the vendored <c>ZStd.cs</c>: the stream and context
/// wrappers there are unused, and compiling the whole file pulled a module initializer and a lot
/// of dead code into the binary.
/// </summary>
[Serializable]
public class ZStdException : Exception
{
    public ZStdException()
    {
    }

    public ZStdException(string message) : base(message)
    {
    }

    public ZStdException(string message, Exception inner) : base(message, inner)
    {
    }

    public static unsafe ZStdException FromCode(nuint code)
    {
        return new ZStdException(Marshal.PtrToStringUTF8((IntPtr)ZSTD_getErrorName(code))!);
    }

    public static void ThrowIfError(nuint code)
    {
        if (ZSTD_isError(code) != 0)
            throw FromCode(code);
    }
}
