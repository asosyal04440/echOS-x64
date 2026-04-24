# Ek C - Matematik Notlari

Bu ekte, kitap boyunca kullanilan ana formuller toplu verilir.

## 1) CFS vruntime

\[
\Delta v = \frac{\Delta t \cdot W_0}{w_i}
\]

- \(\Delta t\): gercek calisma suresi
- \(W_0\): referans agirlik (nice 0)
- \(w_i\): gorev agirligi

Yorum:

- \(w_i\) buyudukce \(\Delta v\) kuculur
- Bu da gorevin daha sik secilebilmesine imkan verir

## 2) Deadline admission

Tek CPU icin ideal kosul:

\[
\sum_i \frac{C_i}{T_i} \le 1
\]

- \(C_i\): runtime butcesi
- \(T_i\): period

Pratikte guvenlik payi ile sinir daha asagi secilir.

## 3) RR timeslice lineerleme fikri

Basitlestirilmis sekilde:

\[
slice = s_{min} + \alpha(priority) \cdot (s_{max} - s_{min})
\]

Burada \(\alpha\) 0-1 araliginda normalize onceliktir.

## 4) Reclaim ve servis hizi sezgisi

Kaba kuyruk modeli:

\[
\rho = \frac{\lambda}{\mu}
\]

- \(\lambda\): dirty page uretilme hizi
- \(\mu\): writeback servis hizi

\(\rho > 1\) uzun sure kalirsa writeback kuyrugu buyur.

## 5) Lock-free ring kapasite sezgisi

Ring doluluk oranı:

\[
occ = tail - head
\]

Kapasite \(N\) icin `occ >= N` dolu durumudur.

## 6) Basit latency ayrisimi

Toplam gecikme:

\[
L_{total} = L_{queue} + L_{service} + L_{sync}
\]

- \(L_{queue}\): bekleme
- \(L_{service}\): asil is
- \(L_{sync}\): bariyer/atomik/locking etkisi

Bu ayrisim scheduler, io_uring ve reclaim analizlerinde ortak bir dildir.
