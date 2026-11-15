#nullable enable

using System;
using System.Reflection;
using System.Runtime.CompilerServices;
using System.Runtime.ExceptionServices;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading;
using System.Threading.Tasks;

// Every P/Invoke is source-generated and blittable: the runtime marshaller is never needed.
[assembly: DisableRuntimeMarshalling]

namespace Teispace.TeiGeodata;

/// <summary>A failed call: its stable <see cref="ErrorCode"/> and the library's message.</summary>
public sealed class TeiGeodataException : Exception
{
    /// <summary>A failure with this code and message.</summary>
    public TeiGeodataException(ErrorCode code, string message) : base(message)
    {
        Code = code;
    }

    /// <summary>The stable code, the same in every language.</summary>
    public ErrorCode Code { get; }

    /// <summary>The library was built without the feature that has <paramref name="symbol"/>.</summary>
    internal static TeiGeodataException Missing(string symbol) =>
        new(ErrorCode.MissingComponent, $"the loaded library has no {symbol}: it was built without that feature");
}

/// <summary>
/// The native library, loaded once per process: from <see cref="Load"/>, else on the first call
/// from the file <c>TEIGEODATA_LIB</c> names, else the package's <c>runtimes/&lt;rid&gt;/native</c>
/// file or the system's library path. A library of another version is refused.
/// </summary>
public static class Library
{
    /// <summary>The library version this package's generated code calls.</summary>
    public const string Version = "1.0.0";

    internal const string Name = "teigeodata";

    private static readonly object Gate = new();
    private static volatile bool ready;
    private static string? path;
    private static nint loaded;
    private static Exception? failed;

    /// <summary>The file the library was loaded from (or its bare name, when the runtime found
    /// it), null before it is loaded.</summary>
    public static string? LoadedPath => ready ? path : null;

    /// <summary>Loads the native library at <paramref name="file"/> for every later call. Loading
    /// another file once a library is loaded is refused.</summary>
    public static void Load(string file)
    {
        ArgumentNullException.ThrowIfNull(file);
        lock (Gate)
        {
            if (ready)
            {
                if (file == path)
                {
                    return;
                }
                throw new InvalidOperationException($"teigeodata: {path} is already loaded");
            }
            LoadFrom(file);
            failed = null;
        }
    }

    /// <summary>Loads the library on first use; afterwards it costs one volatile read.</summary>
    internal static void Ensure()
    {
        if (!ready)
        {
            LoadDefault();
        }
    }

    private static void LoadDefault()
    {
        lock (Gate)
        {
            if (ready)
            {
                return;
            }
            if (failed is not null)
            {
                ExceptionDispatchInfo.Throw(failed);
            }
            try
            {
                string? env = Environment.GetEnvironmentVariable("TEIGEODATA_LIB");
                LoadFrom(string.IsNullOrEmpty(env) ? null : env);
            }
            catch (Exception e)
            {
                failed = e;
                throw;
            }
        }
    }

    /// <summary>Loads <paramref name="file"/>, or with null lets the runtime probe for the
    /// library, then checks its version. Called with <see cref="Gate"/> held.</summary>
    private static unsafe void LoadFrom(string? file)
    {
        nint handle;
        try
        {
            handle = file is null
                ? NativeLibrary.Load(Name, typeof(Library).Assembly, null)
                : NativeLibrary.Load(file);
        }
        catch (DllNotFoundException e)
        {
            throw new DllNotFoundException(
                $"teigeodata: no native library found ({e.Message}). Install the package for this " +
                "platform's runtime identifier, set TEIGEODATA_LIB to the library file, or call " +
                "Library.Load.", e);
        }
        Resolver.Install();
        loaded = handle;
        TgBytes @out = default;
        int status = Native.tg_version(&@out);
        Interop.Check(status);
        string got = Interop.TakeString(@out);
        if (got != Version)
        {
            throw new InvalidOperationException(
                $"teigeodata: {file ?? Name} is version {got}; this package calls {Version}");
        }
        path = file ?? Name;
        ready = true;
    }

    /// <summary>Points every P/Invoke of this assembly at the loaded library.</summary>
    private static class Resolver
    {
        private static int installed;

        internal static void Install()
        {
            if (Interlocked.Exchange(ref installed, 1) == 0)
            {
                NativeLibrary.SetDllImportResolver(typeof(Library).Assembly, Resolve);
            }
        }

        private static nint Resolve(string name, Assembly assembly, DllImportSearchPath? searchPath) =>
            name == Name ? loaded : 0;
    }
}

/// <summary>The C tg_bytes: a buffer the library allocated, freed by tg_bytes_free.</summary>
[StructLayout(LayoutKind.Sequential)]
internal struct TgBytes
{
#pragma warning disable CS0649 // written by the library
    public nint Ptr;
    public nuint Len;
    public nuint Cap;
#pragma warning restore CS0649
}

/// <summary>A string as UTF-8 for one call: on the stack when it fits, else in native memory
/// freed by <see cref="Dispose"/>. Its pointer needs no pinning. A string that is not valid
/// UTF-16 (a lone surrogate) is refused, not silently replaced.</summary>
internal unsafe ref struct Utf8
{
    /// <summary>The stack buffer each call gives a string.</summary>
    public const int Stack = 256;

    private static readonly UTF8Encoding Strict = new(false, true);

    private byte* heap;

    public Utf8(string? text, Span<byte> stack)
    {
        heap = null;
        Pointer = null;
        Length = 0;
        if (string.IsNullOrEmpty(text))
        {
            return;
        }
        int length;
        if (text.Length <= stack.Length / 3)
        {
            length = Strict.GetBytes(text, stack);
            Pointer = (byte*)Unsafe.AsPointer(ref MemoryMarshal.GetReference(stack));
        }
        else
        {
            int count = Strict.GetByteCount(text);
            if (count <= stack.Length)
            {
                length = Strict.GetBytes(text, stack);
                Pointer = (byte*)Unsafe.AsPointer(ref MemoryMarshal.GetReference(stack));
            }
            else
            {
                heap = (byte*)NativeMemory.Alloc((nuint)count);
                length = Strict.GetBytes(text, new Span<byte>(heap, count));
                Pointer = heap;
            }
        }
        Length = (nuint)length;
    }

    /// <summary>The bytes, null when there are none.</summary>
    public byte* Pointer { get; }

    /// <summary>The number of bytes.</summary>
    public nuint Length { get; }

    public void Dispose()
    {
        if (heap is not null)
        {
            NativeMemory.Free(heap);
            heap = null;
        }
    }
}

/// <summary>A callback's managed function and the exception it threw, if any: an exception may
/// not unwind through the library's frames, so the trampoline keeps it for the caller.</summary>
internal sealed class CallbackBox
{
    public CallbackBox(Delegate function)
    {
        Function = function;
    }

    public Delegate Function { get; }

    public ExceptionDispatchInfo? Error { get; set; }
}

/// <summary>What every generated call shares.</summary>
internal static unsafe class Interop
{
    internal static void Ensure() => Library.Ensure();

    /// <summary>Throws the failure a call returned. The message is read on the same thread,
    /// straight after the call, as the C ABI requires.</summary>
    internal static void Check(int status)
    {
        if (status != 0)
        {
            Throw(status);
        }
    }

    private static void Throw(int status)
    {
        TgBytes message = default;
        Native.tg_last_error(&message);
        throw new TeiGeodataException((ErrorCode)status, TakeString(message));
    }

    /// <summary>Copies a library string into managed memory and frees it.</summary>
    internal static string TakeString(TgBytes bytes)
    {
        if (bytes.Ptr == 0)
        {
            return string.Empty;
        }
        try
        {
            return Encoding.UTF8.GetString((byte*)bytes.Ptr, checked((int)bytes.Len));
        }
        finally
        {
            Native.tg_bytes_free(bytes);
        }
    }

    /// <summary>Copies a library blob into managed memory and frees it: a managed array, so a
    /// view of it keeps it alive, as the garbage collector cannot do for native memory.</summary>
    internal static byte[] TakeBytes(TgBytes bytes)
    {
        if (bytes.Ptr == 0)
        {
            return [];
        }
        try
        {
            int length = checked((int)bytes.Len);
            byte[] copy = GC.AllocateUninitializedArray<byte>(length);
            new ReadOnlySpan<byte>((void*)bytes.Ptr, length).CopyTo(copy);
            return copy;
        }
        finally
        {
            Native.tg_bytes_free(bytes);
        }
    }

    /// <summary>Runs a blocking call on a thread of its own (D-281), so an await never holds a
    /// thread-pool thread for the length of a download or a scan.</summary>
    internal static Task<T> RunBlocking<T>(Func<T> call) =>
        Task.Factory.StartNew(call, CancellationToken.None, TaskCreationOptions.LongRunning, TaskScheduler.Default);

    internal static Task RunBlocking(Action call) =>
        Task.Factory.StartNew(call, CancellationToken.None, TaskCreationOptions.LongRunning, TaskScheduler.Default);
}
