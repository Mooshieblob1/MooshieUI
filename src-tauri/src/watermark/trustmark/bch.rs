// Vendored from Adobe's TrustMark Rust crate (trustmark 0.4.0, MIT License,
// Copyright (c) 2024 Adobe, https://github.com/adobe/trustmark). Kept as-is
// apart from this header; see `watermark/trustmark/mod.rs` for why the crate
// is vendored instead of depended on.
//
// Original header follows.
//
// Copyright 2024 Adobe
// All Rights Reserved.
//
// NOTICE: Adobe permits you to use, modify, and distribute this file in
// accordance with the terms of the Adobe license agreement accompanying
// it.

// This module is a direct port of trustmark's BCH encoding/decoding code[^1].
//
// TODO: Refactor this to be clearer/more maintainable.
//
// [^1]: https://github.com/adobe/trustmark/blob/0a45b8dc4e515068e5d043c9a1086e094814b5d7/python/trustmark/bchecc.py

pub(super) const POLYNOMIAL: u32 = 137;

#[derive(Debug)]
pub(super) struct EccState {
    m: u32,
    t: u32,
    _poly: u32,
    n: Option<u32>,
    ecc_bytes: Option<u32>,
    cyclic_tab: Option<Vec<u32>>,
    syn: Option<Vec<u32>>,
    elp: Option<Vec<u32>>,
    errloc: Option<Vec<u32>>,
    exponents: Option<Vec<u32>>,
    logarithms: Option<Vec<u32>>,
    elp_pre: Option<Vec<u32>>,
    ecc_bits: Option<u32>,
    ecc_buf: Option<Vec<u32>>,
}

#[derive(Debug, Clone)]
struct Polynomial {
    deg: u32,
    c: Option<Vec<u32>>,
}

impl Polynomial {
    fn new(deg: u32) -> Self {
        Self { deg, c: None }
    }
}

// def __init__(self, t, poly):
pub(super) fn bch_init(t: u32, poly: u32) -> EccState {
    //    tmp = poly;
    //    m = 0;
    //    while (tmp >> 1):
    //       tmp =tmp >> 1
    //       m +=1
    let m = poly.ilog2();

    //    self.ECCstate=self.params(m=m,t=t,poly=poly)
    let mut ecc_state = EccState::new(m, t, poly);

    //    self.ECCstate.n=pow(2,m)-1
    ecc_state.n = Some(2_u32.pow(m) - 1);
    //    words = self.ceilop(m*t,32)
    let words = (m * t).div_ceil(32);
    //    self.ECCstate.ecc_bytes = self.ceilop(m*t,8)
    ecc_state.ecc_bytes = Some((m * t).div_ceil(8));
    //    self.ECCstate.cyclic_tab=[0]*(words*1024)
    ecc_state.cyclic_tab = Some(vec![0; (words * 1024).try_into().unwrap()]);
    //    self.ECCstate.syn=[0]*(2*t)
    ecc_state.syn = Some(vec![0; (2 * t).try_into().unwrap()]);
    //    self.ECCstate.elp=[0]*(t+1)
    ecc_state.elp = Some(vec![0; (t + 1).try_into().unwrap()]);
    //    self.ECCstate.errloc=[0] * t
    ecc_state.errloc = Some(vec![0; t.try_into().unwrap()]);

    //    x=1
    //    k=pow(2,self.deg(poly))
    //    if k != pow(2,self.ECCstate.m):
    //      return -1
    let mut x = 1;
    let k = 2_u32.pow(poly.ilog2());
    if k != 2_u32.pow(ecc_state.m) {
        panic!("k ({k}) should be equal to {}", 2_u32.pow(ecc_state.m));
    }

    //    self.ECCstate.exponents=[0]*(1+self.ECCstate.n)
    ecc_state.exponents = Some(vec![0; (1 + ecc_state.n.unwrap()).try_into().unwrap()]);
    //    self.ECCstate.logarithms=[0]*(1+self.ECCstate.n)
    ecc_state.logarithms = Some(vec![0; (1 + ecc_state.n.unwrap()).try_into().unwrap()]);
    //    self.ECCstate.elp_pre=[0]*(1+self.ECCstate.m)
    ecc_state.elp_pre = Some(vec![0; (1 + ecc_state.m).try_into().unwrap()]);

    //    for i in range(0,self.ECCstate.n):
    //      self.ECCstate.exponents[i]=x
    //      self.ECCstate.logarithms[x]=i
    //      if i and x==1:
    //       return -1
    //      x*= 2
    //      if (x & k):
    //        x=x^poly
    for i in 0..ecc_state.n.unwrap() {
        ecc_state.exponents.as_mut().unwrap()[i as usize] = x;
        ecc_state.logarithms.as_mut().unwrap()[x as usize] = i;
        if i != 0 && x == 1 {
            panic!("i ({i}) was nonzero and x was 1");
        }
        x *= 2;
        if x & k != 0 {
            x ^= poly;
        }
    }

    //    self.ECCstate.logarithms[0]=0
    //    self.ECCstate.exponents[self.ECCstate.n]=1
    ecc_state.logarithms.as_mut().unwrap()[0] = 0;
    ecc_state.exponents.as_mut().unwrap()[ecc_state.n.unwrap() as usize] = 1;

    //    n=0
    //    g=self.polynomial(deg=0)
    //    g.c=[0]*((m*t)+1)
    //    roots=[0]*(self.ECCstate.n+1)
    //    genpoly=[0]*self.ceilop(m*t+1,32)
    let mut g = Polynomial::new(0);
    g.c = Some(vec![0; (m * t + 1).try_into().unwrap()]);
    let mut roots = vec![0; (ecc_state.n.unwrap() + 1).try_into().unwrap()];
    let mut genpoly = vec![0; (m * t + 1).div_ceil(32).try_into().unwrap()];

    //    # enum all roots
    //    for i in range(0,t):
    //       r=2*i+1
    //       for j in range(0,m):
    //         roots[r]=1
    //         r=self.mod(2*r)
    for i in 0..t {
        let mut r = 2 * i + 1;
        for _j in 0..m {
            roots[r as usize] = 1;
            r = (2 * r) % ecc_state.n.unwrap();
        }
    }

    //    # build g(x)
    //    g.deg=0
    //    g.c[0]=1
    g.c.as_mut().unwrap()[0] = 1;
    //    for i in range(0,self.ECCstate.n):
    //      if roots[i]:
    //        r=self.ECCstate.exponents[i]
    //        g.c[g.deg+1]=1
    //        for j in range(g.deg,0,-1):
    //            g.c[j]=self.g_mul(g.c[j],r)^g.c[j-1]
    //        g.c[0]=self.g_mul(g.c[0],r)
    //        g.deg += 1
    for i in 0..ecc_state.n.unwrap() {
        if roots[i as usize] != 0 {
            let r = ecc_state.exponents.as_ref().unwrap()[i as usize];
            g.c.as_mut().unwrap()[(g.deg + 1) as usize] = 1;
            for j in (1..=g.deg).rev() {
                g.c.as_mut().unwrap()[j as usize] = ecc_state
                    .g_mul(g.c.as_ref().unwrap()[j as usize], r)
                    ^ g.c.as_ref().unwrap()[(j - 1) as usize];
            }
            g.c.as_mut().unwrap()[0] = ecc_state.g_mul(g.c.as_ref().unwrap()[0], r);
            g.deg += 1;
        }
    }

    //    # store
    //    n = g.deg+1
    //    i = 0
    let mut n = g.deg + 1;
    let mut i = 0;

    //    while (n>0) :

    //       if n>32:
    //          nbits=32
    //       else:
    //          nbits=n

    //       word=0
    //       for j in range (0,nbits):
    //         if g.c[n-1-j] :
    //             word = word | pow(2,31-j)
    //       genpoly[i]=word
    //       i += 1
    //       n -= nbits
    while n > 0 {
        let nbits = u32::min(n, 32);
        let mut word = 0;
        for j in 0..nbits {
            if g.c.as_ref().unwrap()[(n - 1 - j) as usize] != 0 {
                word |= 2_u32.pow(31 - j);
            }
        }
        genpoly[i] = word;
        i += 1;
        n -= nbits;
    }
    //    self.ECCstate.ecc_bits=g.deg
    ecc_state.ecc_bits = Some(g.deg);

    //    self.build_cyclic(genpoly);
    ecc_state.build_cyclic(&genpoly);

    //    sum=0
    //    aexp=0
    //    for i in range(0,m):
    //      for j in range(0,m):
    //        sum = sum ^ self.g_pow(i*pow(2,j))
    //      if sum:
    //        aexp=self.ECCstate.exponents[i]
    //        break
    let mut sum = 0;
    let mut aexp = 0;
    for i in 0..m {
        for j in 0..m {
            sum ^= ecc_state.g_pow((i * 2_u32.pow(j)) as usize)
        }
        if sum != 0 {
            aexp = ecc_state.exponents.as_ref().unwrap()[i as usize];
            break;
        }
    }

    //    x=0
    //    precomp=[0] * 31
    //    remaining=m
    let mut x = 0;
    let mut precomp = [0; 31];
    let mut remaining = m;

    //    while (x<= self.ECCstate.n and remaining):
    //       y=self.g_sqrt(x)^x
    //       for i in range(0,2):
    //          r=self.g_log(y)
    //          if (y and (r<m) and not precomp[r]):
    //            self.ECCstate.elp_pre[r]=x
    //            precomp[r]=1
    //            remaining -=1
    //            break
    //          y=y^aexp
    //       x += 1
    while x <= ecc_state.n.unwrap() && remaining != 0 {
        let mut y = ecc_state.g_sqrt(x) ^ x;
        for _i in 0..2 {
            let r = ecc_state.logarithms.as_ref().unwrap()[y as usize];
            if y != 0 && r < m && precomp[r as usize] == 0 {
                ecc_state.elp_pre.as_mut().unwrap()[r as usize] = x;
                precomp[r as usize] = 1;
                remaining -= 1;
                break;
            }
            y ^= aexp;
        }
        x += 1;
    }

    // Commentary: The original function was a class initializer, so we implicitly return the
    // `EccState` we've been building up here.
    ecc_state
}

// def encode(self,data):
pub(super) fn bch_encode(ecc_state: &mut EccState, data: &[u8]) -> Vec<u8> {
    //    datalen=len(data)
    //    l=self.ceilop(self.ECCstate.m*self.ECCstate.t, 32)-1
    let l = (ecc_state.m * ecc_state.t).div_ceil(32) - 1;

    //    ecc= [0]*self.ECCstate.ecc_bytes
    // Commentary: This doesn't seem to be used, so I'm deleting it.

    //    ecc_max_words=self.ceilop(31*64, 32)
    //    r = [0]*ecc_max_words
    let ecc_max_words: usize = (31_usize * 64).div_ceil(32);
    let mut r = vec![0; ecc_max_words];

    //    tab0idx=0
    //    tab1idx=tab0idx+256*(l+1)
    //    tab2idx=tab1idx+256*(l+1)
    //    tab3idx=tab2idx+256*(l+1)
    let tab0idx = 0;
    let tab1idx = tab0idx + 256 * (l + 1);
    let tab2idx = tab1idx + 256 * (l + 1);
    let tab3idx = tab2idx + 256 * (l + 1);

    //    mlen=int(datalen/4) # how many whole words
    //    offset=0
    let mut mlen = data.len() / 4;
    let mut offset = 0;
    //    while (mlen>0):
    //       w=self.load4bytes(data[offset:(offset+4)])
    //       w=w^r[0]
    //       p0=tab0idx+(l+1)*((w>>0) & 0xff)
    //       p1=tab1idx+(l+1)*((w>>8) & 0xff)
    //       p2=tab2idx+(l+1)*((w>>16) & 0xff)
    //       p3=tab3idx+(l+1)*((w>>24) & 0xff)

    //       for i in range(0,l):
    //         r[i]=r[i+1] ^ self.ECCstate.cyclic_tab[p0+i] ^ self.ECCstate.cyclic_tab[p1+i] ^ self.ECCstate.cyclic_tab[p2+i] ^ self.ECCstate.cyclic_tab[p3+i]

    //       r[l] = self.ECCstate.cyclic_tab[p0+l]^self.ECCstate.cyclic_tab[p1+l]^self.ECCstate.cyclic_tab[p2+l]^self.ECCstate.cyclic_tab[p3+l];
    //       mlen -=1
    //       offset +=4
    while mlen > 0 {
        let mut w = u32::from_be_bytes(data[offset..offset + 4].try_into().unwrap());
        w ^= r[0];
        let p0 = tab0idx + (l + 1) * (w & 0xff);
        let p1 = tab1idx + (l + 1) * ((w >> 8) & 0xff);
        let p2 = tab2idx + (l + 1) * ((w >> 16) & 0xff);
        let p3 = tab3idx + (l + 1) * ((w >> 24) & 0xff);
        let cyclic_tab = ecc_state.cyclic_tab.as_ref().unwrap();
        for i in 0..l {
            r[i as usize] = r[(i + 1) as usize]
                ^ cyclic_tab[(p0 + i) as usize]
                ^ cyclic_tab[(p1 + i) as usize]
                ^ cyclic_tab[(p2 + i) as usize]
                ^ cyclic_tab[(p3 + i) as usize];
        }
        r[l as usize] = cyclic_tab[(p0 + l) as usize]
            ^ cyclic_tab[(p1 + l) as usize]
            ^ cyclic_tab[(p2 + l) as usize]
            ^ cyclic_tab[(p3 + l) as usize];
        mlen -= 1;
        offset += 4;
    }

    //    data=data[offset:]
    //    leftdata=len(data)
    let data = &data[offset..];
    let mut leftdata = data.len();

    //    ecc=r
    //    posn=0
    let mut ecc = r;
    let mut posn = 0;
    //    while (leftdata):
    //        tmp=data[posn]
    //        posn += 1
    //        pidx = (l+1)*(((ecc[0] >> 24)^(tmp)) & 0xff)
    //        for i in range(0,l):
    //           ecc[i]=(((ecc[i] << 8)&0xffffffff)|ecc[i+1]>>24)^(self.ECCstate.cyclic_tab[pidx])
    //           pidx += 1
    //        ecc[l]=((ecc[l] << 8)&0xffffffff)^(self.ECCstate.cyclic_tab[pidx])
    //        leftdata -= 1
    while leftdata != 0 {
        let tmp = data[posn];
        let cyclic_tab = ecc_state.cyclic_tab.as_ref().unwrap();
        posn += 1;
        let pidx = (l + 1) * (((ecc[0] >> 24) ^ (tmp as u32)) & 0xff);
        for i in 0..l {
            ecc[i as usize] = ((ecc[i as usize] << 8) | (ecc[(i + 1) as usize] >> 24))
                ^ cyclic_tab[pidx as usize];
        }
        ecc[l as usize] = (ecc[l as usize] << 8) ^ cyclic_tab[pidx as usize];
        leftdata -= 1;
    }

    //    self.ECCstate.ecc_buf=ecc
    //    eccout=[]
    ecc_state.ecc_buf = Some(ecc.clone());
    let mut eccout: Vec<u8> = vec![];
    //    for e in r:
    //       eccout.append((e >> 24) & 0xff)
    //       eccout.append((e >> 16) & 0xff)
    //       eccout.append((e >> 8) & 0xff)
    //       eccout.append((e >> 0) & 0xff)
    // Commentary: In the Python version, `ecc` and `r` reference the same value.
    for e in ecc {
        eccout.extend(e.to_be_bytes())
    }

    //    eccout=eccout[0:self.ECCstate.ecc_bytes]
    eccout.truncate(ecc_state.ecc_bytes.unwrap().try_into().unwrap());

    //    eccbytes=(bytearray(bytes(eccout)))
    //    return eccbytes
    eccout
}

// def decode(self,data,recvecc):
pub(super) fn bch_decode(ecc_state: &mut EccState, data: &mut [u8], recvecc: &[u8]) -> u8 {
    //   calc_ecc=self.encode(data)
    let _calc_ecc = bch_encode(ecc_state, data);

    //   self.ECCstate.errloc=[]
    ecc_state.errloc = Some(vec![]);

    //   ecclen=len(recvecc)
    //   mlen=int(ecclen/4) # how many whole words
    //   eccbuf=[]
    //   offset=0
    let ecclen = recvecc.len();
    let mut mlen = ecclen / 4;
    let mut eccbuf = vec![];
    let mut offset = 0;

    //   while (mlen>0):
    while mlen > 0 {
        //      w=self.load4bytes(recvecc[offset:(offset+4)])
        //      eccbuf.append(w)
        //      offset+=4
        //      mlen -=1
        let w = u32::from_be_bytes(recvecc[offset..offset + 4].try_into().unwrap());
        eccbuf.push(w);
        offset += 4;
        mlen -= 1;
    }

    //   recvecc=recvecc[offset:]
    //   leftdata=len(recvecc)
    let mut recvecc = recvecc[offset..].to_vec();
    let leftdata = recvecc.len();

    //   if leftdata>0: #pad it to 4
    if leftdata > 0 {
        //     recvecc=recvecc+bytes([0]*(4-leftdata))
        //     w=self.load4bytes(recvecc)
        //     eccbuf.append(w)
        recvecc.extend(std::iter::repeat_n(0, 4 - leftdata));
        let w = u32::from_be_bytes(recvecc.clone().try_into().unwrap());
        eccbuf.push(w);
    }

    //   eccwords=self.ceilop(self.ECCstate.m*self.ECCstate.t, 32)
    let eccwords = (ecc_state.m * ecc_state.t).div_ceil(32);

    //   sum=0
    let mut sum = 0;
    //   for i in range(0,eccwords):
    for i in 0..eccwords {
        //      self.ECCstate.ecc_buf[i] = self.ECCstate.ecc_buf[i] ^ eccbuf[i]
        //      sum = sum | self.ECCstate.ecc_buf[i]
        ecc_state.ecc_buf.as_mut().unwrap()[i as usize] ^= eccbuf[i as usize];
        sum |= ecc_state.ecc_buf.as_ref().unwrap()[i as usize];
    }
    //   if sum==0:
    //     return 0 # no bit flips
    if sum == 0 {
        return 0;
    }

    //   s=self.ECCstate.ecc_bits
    //   t=self.ECCstate.t
    //   syn=[0]*(2*t)
    let mut s: i32 = ecc_state.ecc_bits.unwrap() as i32;
    let t = ecc_state.t;
    let mut syn = vec![0; 2 * (t as usize)];

    //   m= s & 31
    let m = s & 31;

    //   synbuf=self.ECCstate.ecc_buf
    let mut synbuf = ecc_state.ecc_buf.clone().unwrap();

    //   if (m):
    //     synbuf[int(s/32)] = synbuf[int(s/32)] & ~(pow(2,32-m)-1)
    if m != 0 {
        synbuf[(s / 32) as usize] &= !(2_u32.pow(32 - (m as u32)) - 1);
    }

    //   synptr=0
    let mut synptr = 0;
    //   while(s>0 or synptr==0):
    while s > 0 || synptr == 0 {
        //       poly=synbuf[synptr]
        //       synptr += 1
        //       s-= 32
        let mut poly = synbuf[synptr];
        synptr += 1;
        s -= 32;
        //       while (poly):
        while poly != 0 {
            //          i=self.deg(poly)
            let i = poly.ilog2();
            //          for j in range(0,(2*t),2):
            for j in (0..2 * t).step_by(2) {
                //            syn[j]=syn[j] ^ self.g_pow((j+1)*(i+s))
                syn[j as usize] ^= ecc_state.g_pow(((j + 1) * (((i as i32) + s) as u32)) as usize);
            }
            //          poly = poly ^ pow(2,i)
            poly ^= 2_u32.pow(i);
        }
    }

    //   for i in range(0,t):
    //      syn[2*i+1]=self.g_sqrt(syn[i])
    for i in 0..t {
        syn[(2 * i + 1) as usize] = ecc_state.g_sqrt(syn[i as usize]);
    }

    //   n=self.ECCstate.n
    //   t=self.ECCstate.t
    //   pp=-1
    //   pd=1
    let n = ecc_state.n.unwrap();
    let t = ecc_state.t;
    let mut pp: i32 = -1;
    let mut pd = 1;

    //   pelp=self.polynomial(deg=0)
    //   pelp.deg=0
    //   pelp.c= [0]*(2*t)
    //   pelp.c[0]=1
    let mut pelp = Polynomial::new(0);
    pelp.deg = 0;
    pelp.c = Some(vec![0; (2 * t) as usize]);
    pelp.c.as_mut().unwrap()[0] = 1;

    //   elp=self.polynomial(deg=0)
    //   elp.c= [0]*(2*t)
    //   elp.c[0]=1
    let mut elp = Polynomial::new(0);
    elp.c = Some(vec![0; (2 * t) as usize]);
    elp.c.as_mut().unwrap()[0] = 1;

    //   d=syn[0]
    let mut d = syn[0];

    //   elp_copy=self.polynomial(deg=0)
    let mut elp_copy;
    //   for i in range(0,t):
    for i in 0..t {
        //      if (elp.deg>t):
        //          break
        if elp.deg > t {
            break;
        }
        //      if d:
        if d != 0 {
            //         k=2*i-pp
            //         elp_copy=deepcopy(elp)
            //         tmp=self.g_log(d)+n-self.g_log(pd)
            let k = (2 * (i as i32) - pp) as u32;
            elp_copy = elp.clone();
            let tmp = ecc_state.g_log(d) + n - ecc_state.g_log(pd);
            //         for j in range(0,(pelp.deg+1)):
            for j in 0..pelp.deg + 1 {
                //           if (pelp.c[j]):
                //             l=self.g_log(pelp.c[j])
                //             elp.c[j+k]=elp.c[j+k] ^ self.g_pow(tmp+l)
                if pelp.c.as_ref().unwrap()[j as usize] != 0 {
                    let l = ecc_state.g_log(pelp.c.as_ref().unwrap()[j as usize]);
                    elp.c.as_mut().unwrap()[(j + k) as usize] ^=
                        ecc_state.g_pow((tmp + l) as usize);
                }
            }

            //         tmp=pelp.deg+k
            let tmp = pelp.deg + k;
            //         if tmp>elp.deg:
            if tmp > elp.deg {
                //             elp.deg=tmp
                //             pelp=deepcopy(elp_copy)
                //             pd=d
                //             pp=2*i
                elp.deg = tmp;
                pelp = elp_copy.clone();
                pd = d;
                pp = (2 * i) as i32;
            }
        }
        //      if (i<t-1):
        if i < t - 1 {
            //          d=syn[2*i+2]
            //          for j in range(1,(elp.deg+1)):
            //              d = d ^ self.g_mul(elp.c[j],syn[2*i+2-j])
            d = syn[(2 * i + 2) as usize];
            for j in 1..elp.deg + 1 {
                d ^= ecc_state.g_mul(
                    elp.c.as_ref().unwrap()[j as usize],
                    syn[(2 * i + 2 - j) as usize],
                );
            }
        }
    }
    //   self.ECCstate.elp=elp
    //   Commentary: `ecc_state.elp` is an `Option<Vec<u32>>`, I'm going to leave this assignment
    //   out for now and see where it gets me. That means that below, I will refer to `elp` instead
    //   of `ecc_state.elp`.

    //   nroots = self.getroots(len(data),self.ECCstate.elp)
    //   datalen=len(data)
    //   nbits=(datalen*8)+self.ECCstate.ecc_bits
    let nroots = ecc_state.getroots(data.len() as u32, &elp);
    // Python version of getroots() returned -1 on failure. Then, when we range over (0,-1), no
    // iteration would occur.
    // Rust version returns u32::MAX. If we try to iterate over 0..u32::MAX, we'll try to access an
    // element from ecc_state.errloc, which is empty.
    // Hence, we check for the sentinal value here.
    if nroots == u32::MAX {
        return u8::MAX;
    }
    let datalen = data.len() as u32;
    let nbits = (datalen * 8) + ecc_state.ecc_bits.unwrap();

    //   for i in range(0,nroots):
    //       if self.ECCstate.errloc[i] >= nbits:
    //         return -1
    //       self.ECCstate.errloc[i]=nbits-1-self.ECCstate.errloc[i]
    //       self.ECCstate.errloc[i]=(self.ECCstate.errloc[i] & ~7) | (7-(self.ECCstate.errloc[i] & 7))
    for i in 0..nroots {
        if ecc_state.errloc.as_mut().unwrap()[i as usize] >= nbits {
            return u8::MAX;
        }
        ecc_state.errloc.as_mut().unwrap()[i as usize] =
            nbits - 1 - ecc_state.errloc.as_ref().unwrap()[i as usize];
        ecc_state.errloc.as_mut().unwrap()[i as usize] =
            (ecc_state.errloc.as_ref().unwrap()[i as usize] & !7)
                | (7 - (ecc_state.errloc.as_ref().unwrap()[i as usize] & 7))
    }

    //   for bitflip in self.ECCstate.errloc:
    //       byte= int (bitflip / 8)
    //       bit = pow(2,(bitflip & 7))
    //       if bitflip < (len(data)+len(recvecc))*8:
    //         if byte<len(data):
    //           data[byte] = data[byte] ^ bit
    //         else:
    //           recvecc[byte - len(data)] = recvecc[byte - len(data)] ^ bit
    for bitflip in ecc_state.errloc.as_ref().unwrap() {
        let byte = bitflip / 8;
        let bit = 2_u32.pow(bitflip & 7);
        if (*bitflip as usize) < (data.len() + recvecc.len()) * 8 {
            if (byte as usize) < data.len() {
                data[byte as usize] ^= bit as u8;
            } else {
                recvecc[(byte - data.len() as u32) as usize] ^= bit as u8;
            }
        }
    }

    nroots as u8
}

impl EccState {
    fn new(m: u32, t: u32, _poly: u32) -> Self {
        Self {
            m,
            t,
            _poly,
            n: None,
            ecc_bytes: None,
            cyclic_tab: None,
            syn: None,
            elp: None,
            errloc: None,
            exponents: None,
            logarithms: None,
            elp_pre: None,
            ecc_bits: None,
            ecc_buf: None,
        }
    }

    // def build_cyclic(self, g):
    fn build_cyclic(&mut self, g: &[u32]) {
        //   l=self.ceilop(self.ECCstate.m*self.ECCstate.t, 32)
        let l = (self.m * self.t).div_ceil(32);

        //   plen=self.ceilop(self.ECCstate.ecc_bits+1,32)
        //   ecclen=self.ceilop(self.ECCstate.ecc_bits,32)
        let plen = (self.ecc_bits.unwrap() + 1).div_ceil(32);
        let ecclen = self.ecc_bits.unwrap().div_ceil(32);

        //   self.ECCstate.cyclic_tab = [0] * 4*256*l
        self.cyclic_tab = Some(vec![0; (4 * 256 * l).try_into().unwrap()]);

        //   for i in range(0,256):
        for i in 0..256 {
            //      for b in range(0,4):
            for b in 0..4 {
                //        offset= (b*256+i)*l
                //        data = i << 8*b
                let offset = (b * 256 + i) * l;
                let mut data = i << (8 * b);

                //        while (data):
                while data != 0 {
                    //          d=self.deg(data)
                    let d = data.ilog2();
                    //          data = data ^ (g[0] >> (31-d))
                    data ^= g[0] >> (31 - d);
                    //          for j in range(0,ecclen):
                    for j in 0..ecclen {
                        //             if d<31:
                        //               hi=(g[j] << (d+1)) & 0xffffffff
                        //             else:
                        //               hi=0
                        let hi = if d < 31 { g[j as usize] << (d + 1) } else { 0 };
                        //             if j+1 < plen:
                        //               lo= g[j+1] >> (31-d)
                        //             else:
                        //               lo= 0
                        let lo = if j + 1 < plen {
                            g[(j + 1) as usize] >> (31 - d)
                        } else {
                            0
                        };
                        //             self.ECCstate.cyclic_tab[j+offset] = self.ECCstate.cyclic_tab[j+offset] ^ (hi | lo)
                        self.cyclic_tab.as_mut().unwrap()[(j + offset) as usize] ^= hi | lo;
                    }
                }
            }
        }
    }

    // def g_mul(self, a,b):

    //    if (a>0 and b>0):
    //      res=self.mod(self.ECCstate.logarithms[a]+self.ECCstate.logarithms[b])
    //      return (self.ECCstate.exponents[res])
    //    else:
    //      return 0
    fn g_mul(&self, a: u32, b: u32) -> u32 {
        if a > 0 && b > 0 {
            let res = (self.logarithms.as_ref().unwrap()[a as usize]
                + self.logarithms.as_ref().unwrap()[b as usize])
                % self.n.unwrap();
            self.exponents.as_ref().unwrap()[res as usize]
        } else {
            0
        }
    }

    // def g_pow(self, i):
    //    return self.ECCstate.exponents[self.modn(i)]
    fn g_pow(&self, i: usize) -> u32 {
        self.exponents.as_ref().unwrap()[self.modn(i.try_into().unwrap()) as usize]
    }

    // def modn(self, v):
    //    n=self.ECCstate.n
    //    while (v>=n):
    //       v -= n
    //       v = (v & n) + (v >> self.ECCstate.m)
    //    return v
    fn modn(&self, mut v: u32) -> u32 {
        while v >= self.n.unwrap() {
            v -= self.n.unwrap();
            v = (v & self.n.unwrap()) + (v >> self.m);
        }
        v
    }

    // def g_sqrt(self, a):
    //    if a:
    //       return self.ECCstate.exponents[self.mod(2*self.ECCstate.logarithms[a])]
    //    else:
    //       return 0
    fn g_sqrt(&self, a: u32) -> u32 {
        if a != 0 {
            self.exponents.as_ref().unwrap()
                [(2 * self.logarithms.as_ref().unwrap()[a as usize] % self.n.unwrap()) as usize]
        } else {
            0
        }
    }

    // def g_log(self, x):
    //    return self.ECCstate.logarithms[x]
    fn g_log(&self, a: u32) -> u32 {
        self.logarithms.as_ref().unwrap()[a as usize]
    }

    // def mod(self, v):
    //     if v<self.ECCstate.n:
    //       return v
    //     else:
    //       return v-self.ECCstate.n
    fn r#mod(&self, v: u32) -> u32 {
        if v < self.n.unwrap() {
            v
        } else {
            v - self.n.unwrap()
        }
    }

    // def g_div(self,a,b):
    //    if a:
    //      return self.ECCstate.exponents[self.mod(self.ECCstate.logarithms[a]+self.ECCstate.n-self.ECCstate.logarithms[b])]
    //    else:
    //      return 0
    fn g_div(&self, a: u32, b: u32) -> u32 {
        if a != 0 {
            self.exponents.as_ref().unwrap()[self.r#mod(
                self.logarithms.as_ref().unwrap()[a as usize] + self.n.unwrap()
                    - self.logarithms.as_ref().unwrap()[b as usize],
            ) as usize]
        } else {
            0
        }
    }

    // def getroots(self, k, poly):
    fn getroots(&mut self, mut k: u32, poly: &Polynomial) -> u32 {
        //    roots=[]
        let mut roots = vec![];

        //    if poly.deg>2:
        if poly.deg > 2 {
            //       k=k*8+self.ECCstate.ecc_bits
            k = k * 8 + self.ecc_bits.unwrap();

            //       rep=[0]*(self.ECCstate.t*2)
            //       d=poly.deg
            //       l=self.ECCstate.n-self.g_log(poly.c[poly.deg])
            let mut rep: Vec<i32> = vec![0; (self.t * 2) as usize];
            let d = poly.deg;
            let l = self.n.unwrap() - self.g_log(poly.c.as_ref().unwrap()[poly.deg as usize]);
            //       for i in range(0,d):
            for i in 0..d {
                //          if poly.c[i]:
                //            rep[i]=self.mod(self.g_log(poly.c[i])+l)
                //          else:
                //            rep[i]=-1
                if poly.c.as_ref().unwrap()[i as usize] != 0 {
                    rep[i as usize] =
                        self.r#mod(self.g_log(poly.c.as_ref().unwrap()[i as usize]) + l) as i32;
                } else {
                    rep[i as usize] = -1;
                }
            }

            //       rep[poly.deg]=0
            //       syn0=self.g_div(poly.c[0],poly.c[poly.deg])
            rep[poly.deg as usize] = 0;
            let syn0 = self.g_div(
                poly.c.as_ref().unwrap()[0],
                poly.c.as_ref().unwrap()[poly.deg as usize],
            );
            //       for i in range(self.ECCstate.n-k+1, self.ECCstate.n+1):
            for i in self.n.unwrap() - k + 1..self.n.unwrap() + 1 {
                //           syn=syn0
                let mut syn = syn0;
                //           for j in range(1,poly.deg+1):
                //               m=rep[j]
                //               if m>=0:
                //                  syn = syn ^ self.g_pow(m+j*i)
                for j in 1..poly.deg + 1 {
                    let m = rep[j as usize];
                    if m >= 0 {
                        syn ^= self.g_pow(((m as u32) + j * i) as usize);
                    }
                }
                //           if syn==0:
                //               roots.append(self.ECCstate.n-i)
                //               if len(roots)==poly.deg:
                //                   break
                if syn == 0 {
                    roots.push(self.n.unwrap() - i);
                    if roots.len() == poly.deg as usize {
                        break;
                    }
                }
            }
            //       if len(roots)<poly.deg:
            //           # not enough roots to correct
            //           self.ECCstate.errloc=[]
            //           return -1
            if roots.len() < poly.deg as usize {
                self.errloc = Some(vec![]);
                return u32::MAX;
            }
        }

        //    if poly.deg==1:
        //       if (poly.c[0]):
        //          roots.append(self.mod(self.ECCstate.n-self.ECCstate.logarithms[poly.c[0]]+self.ECCstate.logarithms[poly.c[1]]) )
        if poly.deg == 1 && poly.c.as_ref().unwrap()[0] != 0 {
            roots.push(self.r#mod(
                self.n.unwrap()
                    - self.logarithms.as_ref().unwrap()[poly.c.as_ref().unwrap()[0] as usize]
                    + self.logarithms.as_ref().unwrap()[poly.c.as_ref().unwrap()[1] as usize],
            ));
        }

        //    if poly.deg==2:
        if poly.deg == 2 {
            //       if (poly.c[0] and poly.c[1]):
            if poly.c.as_ref().unwrap()[0] != 0 && poly.c.as_ref().unwrap()[1] != 0 {
                //          l0=self.ECCstate.logarithms[poly.c[0]]
                //          l1=self.ECCstate.logarithms[poly.c[1]]
                //          l2=self.ECCstate.logarithms[poly.c[2]]
                let l0 = self.logarithms.as_ref().unwrap()[poly.c.as_ref().unwrap()[0] as usize];
                let l1 = self.logarithms.as_ref().unwrap()[poly.c.as_ref().unwrap()[1] as usize];
                let l2 = self.logarithms.as_ref().unwrap()[poly.c.as_ref().unwrap()[2] as usize];

                //          u=self.g_pow(l0+l2+2*(self.ECCstate.n-l1))
                //          r=0
                //          v=u
                let u = self.g_pow((l0 + l2 + 2 * (self.n.unwrap() - l1)) as usize);
                let mut r = 0;
                let mut v = u;
                //          while (v):
                //             i=self.deg(v)
                //             r = r ^ self.ECCstate.elp_pre[i]
                //             v = v ^ pow(2,i)
                while v != 0 {
                    let i = v.ilog(2);
                    r ^= self.elp_pre.as_ref().unwrap()[i as usize];
                    v ^= 2_u32.pow(i);
                }
                //          if self.g_sqrt(r)^r == u:
                //             roots.append(self.modn(2*self.ECCstate.n-l1-self.ECCstate.logarithms[r]+l2))
                //             roots.append(self.modn(2*self.ECCstate.n-l1-self.ECCstate.logarithms[r^1]+l2))
                if self.g_sqrt(r) ^ r == u {
                    roots.push(self.modn(
                        2 * self.n.unwrap() - l1 - self.logarithms.as_ref().unwrap()[r as usize]
                            + l2,
                    ));
                    roots.push(self.modn(
                        2 * self.n.unwrap()
                            - l1
                            - self.logarithms.as_ref().unwrap()[(r ^ 1) as usize]
                            + l2,
                    ));
                }
            }
        }

        //    self.ECCstate.errloc=roots
        //    return len(roots)
        let lenroots = roots.len() as u32;
        self.errloc = Some(roots);
        lenroots
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init() {
        let ecc_state = bch_init(5, 137);
        assert_eq!(ecc_state.m, 7);
        assert_eq!(ecc_state.t, 5);
        assert_eq!(ecc_state._poly, 137);
        assert_eq!(ecc_state.n, Some(127));
        assert_eq!(ecc_state.ecc_bytes, Some(5));
        assert_eq!(
            ecc_state.cyclic_tab.as_ref().map(|x| &x[..5]),
            Some([0_u32, 0, 2498495642, 3758096384, 3174305199].as_slice())
        );
        assert_eq!(
            ecc_state.cyclic_tab.as_ref().map(|x| &x[x.len() - 3..]),
            Some([0, 1839291269, 3758096384].as_slice())
        );
        assert_eq!(
            ecc_state.syn.as_ref().map(|x| &x[..]),
            Some([0; 10].as_slice())
        );
        assert_eq!(
            ecc_state.elp.as_ref().map(|x| &x[..]),
            Some([0; 6].as_slice())
        );
        assert_eq!(
            ecc_state.errloc.as_ref().map(|x| &x[..]),
            Some([0; 5].as_slice())
        );
        assert_eq!(
            ecc_state.exponents.as_ref().map(|x| &x[..10]),
            Some([1, 2, 4, 8, 16, 32, 64, 9, 18, 36].as_slice())
        );
        assert_eq!(
            ecc_state.logarithms.as_ref().map(|x| &x[..10]),
            Some([0, 0, 1, 31, 2, 62, 32, 103, 3, 7].as_slice())
        );
        assert_eq!(
            ecc_state.elp_pre.as_ref().map(|x| &x[..]),
            Some([0, 16, 18, 102, 22, 40, 110, 0].as_slice())
        );
        assert_eq!(ecc_state.ecc_bits, Some(35),);
    }

    #[test]
    fn encode_zeros() {
        let mut ecc_state = bch_init(8, 137);
        let ecc = bch_encode(&mut ecc_state, &[0, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(ecc, &[0, 0, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn encode_data() {
        let mut ecc_state = bch_init(4, 137);
        let ecc = bch_encode(&mut ecc_state, &[133, 20, 228, 249, 11, 172, 165, 151, 0]);
        assert_eq!(ecc, &[115, 32, 10, 0]);
    }
}
