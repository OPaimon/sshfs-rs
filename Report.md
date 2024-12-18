# sshfs报告

## 1. 项目概述

### 1.1 项目背景
一个基于ssh协议和fuse的远程文件系统

### 1.2 项目目标
- 实现一个简单的sftp服务器，实现一些被认为需要的文件操作功能，以及一个简单的鉴权系统和审计系统
- 实现一个简单的客户端，实现文件和文件夹的上下传功能，同时可以挂载在fs上(通过fuse)

## 2. 设计思路
### 技术选型
出于一些私心(指想学学Rust)，才从三个题目里选的这个题目，所以语言自然是Rust
大概调查了下，Rust的fuse相关库中，Fuser 是更新也是在维护的一个于是这么选择
而ssh相关库里 服务端我一开始只找到了russh()
而编写客户端的时候突然发现有更高层次的客户端库(于是快乐选用了)
这大概就是主要的两个方面(Fuse和ssh)的两边的考虑过程
数据库单纯就是觉得没必要整别的玩意就直接sqlite了()
很多crate都是来自llm的推荐(赞颂llm)
其实其他的文件传输协议也不是没考虑过 (比如我去看了眼webdav) 然后看了会还是觉得ssh最符合要求(x

### 服务端
一个简单的sftp服务端 使用russh实现 简单来说就是简单的一个账号系统(只实现了账号密码登录)+一个响应sftp请求并执行对应操作的服务端(残缺版)。为了方便数据库直接全局单例了(虽然一开始想着学习下怎么测试驱动就搞了个Mock版的)，审计直接打日志顺便进数据库。顺便考虑到场景(教学用，那肯定也没必要让学生啥都看 就还做了个类似chroot的实现 (而且把服务器的根目录挂载到客户端的某个文件夹也太怪了))

### 客户端
一开始本来想和服务端一样拿russh的后来发现有更高层次的crate所以直接愉快用了。
然后fuse方面直接调库也是只要实现对应的trait就好了，整体来看就是把fuse请求转成sftp请求

## 3. 实现过程

整体的话大概是先实现的服务端再实现的客户端(虽然这两明明可以并行做的)
然后过程中就是深感自己啥也不懂(然后差点就跌入设计模式的深渊了(比如看见了依赖注入，然后啥都想注入注入()))，最开始数据库还有鉴权那块我重写了三次就是为了这玩意()

然后开始做服务端这边的文件操作映射(指响应sftp请求)，说白了就是实现对应的trait啦()，然后还先做了个虚拟路径和真实路径的转化甚至整了(很多)个结构体出来(其实这是我第一次尝试涉足OOP(x)，然后又经典的拿着锤子看啥都是钉子了())中间就是先拿着现成的sftp客户端做了做测试(所以还是辜负了单元测试呜呜呜)。

做了这块以后就暂时先告别了服务端了，然后开始客户端的编写

先去看了眼fuser的例子(然后还抄了个挂载选项(x))

然后就是直接开干了，(这一步我甚至是用的另一个wsl虚拟机(指arch)的ssh，而不是自己实现的服务端(因为觉得做的太烂了，而且真的确实有点问题在很影响服务端实现)来测试)，先把ssh连接跑通，然后写了一些我觉得必要的文件操作以后觉得差不多了，然后就开始服务端客户端一起跑开始测试，然后一堆panic慢慢修。修到觉得差不多正常的功能能用了就算结束

接着开始写审记(其实中间怕是过了一周多了)，打文件操作日志放在数据库里，找了找觉得tracing的layer最适合()

接着开始拼cli工具需要的args()直接使用clap用llm生成了下感觉挺合适()

## 4. 优缺点分析

### 4.1 优点
使用了 ~~伟大(bushi)~~ 的rust来编写，至少性能问题不会特别大
多少做了点测试，还是有点可靠性的(真的吗？)

### 4.2 缺点
想一出是一出，前面想着测试驱动后面直接忘了

服务端那边实现的时候意思精分 文件夹存虚拟目录 文件存真实目录

拿着锤子看啥都是钉子然后可能在一些不太合适的地方应用了不合适的东西还浪费了时间()

有一堆unwarp的雷放着还没排()

没测试过多用户会咋样，大概率会出问题(某个被注释掉的check_req_done那边绝对会出问题)

有很多预先想做的东西最后还是没做(看那一堆unused就知道)

## 5. 心路历程
上面其实应该已经有很多心路历程了(主要是拖延症导致没啥时间写报告所以不整理思绪我就这样)

### 5.1 项目启动
最开始的时候，我大概是抱着“要去做一些从来没做过的事情去做的”
比如试试无GC的语言（c先不论，其他我写过的语言还真是都带gc的(Python, Fsharp 之类的)）试试测试驱动开发 试试什么叫OOP 试试去做写一些和系统有交互的工具
总之我是抱着一种期望来开始做这件事情

### 5.2 项目总结
怎么说呢
实际实现的时候就有一种很奇怪的感觉，一种我去做些什么，但其实真正对整体有帮助的只是其中一小部分的感受。很多时候都是写了些会被改掉的东西，有一种超出控制的感觉(你没办法保证你一开始的预想是对的，是合适的)
然后就会有很多想做的东西没去做，有些地方你知道这里绝对会出问题但是你一时不想去解决然后忘掉(x)
最后发现自己只是做了依托能跑的玩意而已
而且我最近的事情比预想的要多(去医院啊，有朋友要来北京啊什么的)，而晚上的时间也不太敢用来写东西了(去医院就是因为发现自己心脏可能不太好)，然后一开始是每天十二点之后会写一个小时(最开始几天)，然后后来不敢熬夜以后配合拖延症我几乎一两周都没有怎么碰这个项目，有时想着出门能写点(也是错觉)

