#pragma once

#include <unknwn.h>

class CClassFactory : public IClassFactory {
public:
    CClassFactory();

    // IUnknown
    IFACEMETHODIMP QueryInterface(_In_ REFIID riid, _Outptr_ void** ppv);
    IFACEMETHODIMP_(ULONG) AddRef();
    IFACEMETHODIMP_(ULONG) Release();

    // IClassFactory
    IFACEMETHODIMP CreateInstance(_In_opt_ IUnknown* pUnkOuter, _In_ REFIID riid, _Outptr_ void** ppv);
    IFACEMETHODIMP LockServer(_In_ BOOL bLock);

private:
    ~CClassFactory();
    LONG _cRef;
};
